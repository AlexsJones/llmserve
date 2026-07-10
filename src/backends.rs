use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    LlamaServer,
    Ollama,
    MlxLm,
    LmStudio,
    Vllm,
    KoboldCpp,
    LocalAi,
    Lemonade,
    FastFlowLm,
}

impl Backend {
    pub fn label(&self) -> &'static str {
        match self {
            Backend::LlamaServer => "llama-server",
            Backend::Ollama => "Ollama",
            Backend::MlxLm => "MLX",
            Backend::LmStudio => "LM Studio",
            Backend::Vllm => "vLLM",
            Backend::KoboldCpp => "KoboldCpp",
            Backend::LocalAi => "LocalAI",
            Backend::Lemonade => "Lemonade",
            Backend::FastFlowLm => "FastFlowLM",
        }
    }

    /// Whether this backend can serve a local model file (GGUF path on disk).
    pub fn can_serve_local_gguf(&self) -> bool {
        matches!(
            self,
            Backend::LlamaServer | Backend::KoboldCpp | Backend::LocalAi | Backend::Lemonade
        )
    }

    /// Whether this backend can serve a local MLX model directory.
    pub fn can_serve_local_mlx(&self) -> bool {
        matches!(self, Backend::MlxLm)
    }

    /// Whether this backend can serve a model from a local file/directory.
    pub fn can_serve_local(&self, format: &crate::models::ModelFormat) -> bool {
        match format {
            crate::models::ModelFormat::Gguf => self.can_serve_local_gguf(),
            crate::models::ModelFormat::Mlx => self.can_serve_local_mlx(),
        }
    }

    /// Why this backend can't serve local files, if applicable.
    pub fn local_serve_reason(&self) -> Option<&'static str> {
        match self {
            Backend::LlamaServer
            | Backend::KoboldCpp
            | Backend::MlxLm
            | Backend::LocalAi
            | Backend::Lemonade => None,
            Backend::Ollama => Some("Ollama uses its own model registry, not local files"),
            Backend::LmStudio => Some("LM Studio manages its own server"),
            Backend::Vllm => Some("vLLM expects HuggingFace model IDs, not local GGUF files"),
            Backend::FastFlowLm => Some("FastFlowLM uses its own NPU-optimized model registry"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetectedBackend {
    pub backend: Backend,
    pub available: bool,
    pub binary_path: Option<String>,
    pub api_url: Option<String>,
}

impl DetectedBackend {
    pub fn status_label(&self) -> &'static str {
        if self.available { "ready" } else { "not found" }
    }
}

fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(800)))
        .build()
        .new_agent()
}

pub fn detect_backends() -> Vec<DetectedBackend> {
    // Several detectors make HTTP probes with an 800ms timeout; run them all
    // concurrently so detection costs one probe's latency, not the sum.
    let detectors: [(Backend, fn() -> DetectedBackend); 9] = [
        (Backend::LlamaServer, detect_llama_server),
        (Backend::Ollama, detect_ollama),
        (Backend::MlxLm, detect_mlx),
        (Backend::LmStudio, detect_lmstudio),
        (Backend::Vllm, detect_vllm),
        (Backend::KoboldCpp, detect_koboldcpp),
        (Backend::LocalAi, detect_localai),
        (Backend::Lemonade, detect_lemonade),
        (Backend::FastFlowLm, detect_fastflowlm),
    ];
    std::thread::scope(|s| {
        detectors
            .map(|(backend, f)| (backend, s.spawn(f)))
            .map(|(backend, h)| {
                h.join().unwrap_or(DetectedBackend {
                    backend,
                    available: false,
                    binary_path: None,
                    api_url: None,
                })
            })
            .into_iter()
            .collect()
    })
}

/// Cross-platform binary lookup. Uses `which` on Unix and `where` on Windows.
fn find_binary(name: &str) -> Option<String> {
    let cmd = if cfg!(windows) { "where" } else { "which" };
    Command::new(cmd)
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
}

/// Returns Ollama model list: Vec<(name, size_bytes)>
pub fn fetch_ollama_models(api_url: &str) -> Vec<(String, u64)> {
    let url = format!("{api_url}/api/tags");
    let agent = http_agent();
    let Ok(mut response) = agent.get(&url).call() else {
        return Vec::new();
    };
    let Ok(body) = response.body_mut().read_to_string() else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Vec::new();
    };
    let Some(models) = json.get("models").and_then(|m| m.as_array()) else {
        return Vec::new();
    };
    models
        .iter()
        .filter_map(|m| {
            let name = m.get("name")?.as_str()?.to_string();
            let size = m.get("size")?.as_u64().unwrap_or(0);
            Some((name, size))
        })
        .collect()
}

fn detect_llama_server() -> DetectedBackend {
    let result = find_binary("llama-server");

    DetectedBackend {
        backend: Backend::LlamaServer,
        available: result.is_some(),
        binary_path: result,
        api_url: None,
    }
}

fn detect_ollama() -> DetectedBackend {
    let url = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".into());
    let agent = http_agent();
    let available = agent.get(&format!("{url}/api/tags")).call().is_ok();

    DetectedBackend {
        backend: Backend::Ollama,
        available,
        binary_path: None,
        api_url: Some(url),
    }
}

fn detect_mlx() -> DetectedBackend {
    if !cfg!(target_os = "macos") {
        return DetectedBackend {
            backend: Backend::MlxLm,
            available: false,
            binary_path: None,
            api_url: None,
        };
    }

    let available = Command::new("python3")
        .args(["-c", "import mlx_lm"])
        .output()
        .ok()
        .is_some_and(|o| o.status.success());

    DetectedBackend {
        backend: Backend::MlxLm,
        available,
        binary_path: None,
        api_url: None,
    }
}

pub fn backend_key(backend: &Backend) -> &'static str {
    match backend {
        Backend::LlamaServer => "llama-server",
        Backend::Ollama => "ollama",
        Backend::MlxLm => "mlx",
        Backend::LmStudio => "lm-studio",
        Backend::Vllm => "vllm",
        Backend::KoboldCpp => "koboldcpp",
        Backend::LocalAi => "localai",
        Backend::Lemonade => "lemonade",
        Backend::FastFlowLm => "fastflowlm",
    }
}

fn detect_lmstudio() -> DetectedBackend {
    let url = std::env::var("LMSTUDIO_HOST").unwrap_or_else(|_| "http://127.0.0.1:1234".into());
    let agent = http_agent();
    let available = agent.get(&format!("{url}/v1/models")).call().is_ok();

    DetectedBackend {
        backend: Backend::LmStudio,
        available,
        binary_path: None,
        api_url: Some(url),
    }
}

/// Fetch the model list from an OpenAI-compatible `models` endpoint:
/// returns Vec<(id, size_bytes)> (size is not exposed by these APIs).
fn fetch_openai_models(api_url: &str, path: &str) -> Vec<(String, u64)> {
    let url = format!("{api_url}{path}");
    let agent = http_agent();
    let Ok(mut response) = agent.get(&url).call() else {
        return Vec::new();
    };
    let Ok(body) = response.body_mut().read_to_string() else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Vec::new();
    };
    let Some(data) = json.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    data.iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            Some((id, 0u64))
        })
        .collect()
}

/// Returns LM Studio model list from its OpenAI-compatible /v1/models endpoint.
pub fn fetch_lmstudio_models(api_url: &str) -> Vec<(String, u64)> {
    fetch_openai_models(api_url, "/v1/models")
}

fn detect_vllm() -> DetectedBackend {
    // Check for running vLLM server first (default port 8000, OpenAI-compatible)
    let url = std::env::var("VLLM_HOST").unwrap_or_else(|_| "http://localhost:8000".into());
    let agent = http_agent();
    let server_running = agent.get(&format!("{url}/v1/models")).call().is_ok();

    // Also check for the vllm binary so we can launch it
    let binary = find_binary("vllm");

    DetectedBackend {
        backend: Backend::Vllm,
        available: server_running || binary.is_some(),
        binary_path: binary,
        api_url: Some(url),
    }
}

fn detect_koboldcpp() -> DetectedBackend {
    let binary = find_binary("koboldcpp");

    // Also check for a running KoboldCpp server (default port 5001)
    let url = std::env::var("KOBOLDCPP_HOST").unwrap_or_else(|_| "http://localhost:5001".into());
    let agent = http_agent();
    let server_running = agent.get(&format!("{url}/api/v1/model")).call().is_ok();

    DetectedBackend {
        backend: Backend::KoboldCpp,
        available: binary.is_some() || server_running,
        binary_path: binary,
        api_url: Some(url),
    }
}

fn detect_lemonade() -> DetectedBackend {
    let binary = find_binary("lemonade");

    // Check for a running Lemonade server (default port 8000)
    let url = std::env::var("LEMONADE_HOST").unwrap_or_else(|_| "http://127.0.0.1:8000".into());
    let agent = http_agent();
    let server_running = agent.get(&format!("{url}/api/v1/health")).call().is_ok();

    DetectedBackend {
        backend: Backend::Lemonade,
        available: binary.is_some() || server_running,
        binary_path: binary,
        api_url: Some(url),
    }
}

/// Returns Lemonade model list from its OpenAI-compatible /api/v1/models endpoint.
pub fn fetch_lemonade_models(api_url: &str) -> Vec<(String, u64)> {
    fetch_openai_models(api_url, "/api/v1/models")
}

fn detect_fastflowlm() -> DetectedBackend {
    let binary = find_binary("flm");

    // Check for a running FastFlowLM server (default port 52625)
    let url = std::env::var("FLM_HOST").unwrap_or_else(|_| "http://127.0.0.1:52625".into());
    let agent = http_agent();
    let server_running = agent.get(&format!("{url}/v1/models")).call().is_ok();

    DetectedBackend {
        backend: Backend::FastFlowLm,
        available: binary.is_some() || server_running,
        binary_path: binary,
        api_url: Some(url),
    }
}

/// Returns FastFlowLM model list from its OpenAI-compatible /v1/models endpoint.
pub fn fetch_fastflowlm_models(api_url: &str) -> Vec<(String, u64)> {
    fetch_openai_models(api_url, "/v1/models")
}

fn detect_localai() -> DetectedBackend {
    // LocalAI: check for binary or running server
    // Default port 8080, but commonly run on other ports to avoid conflicts
    let url = std::env::var("LOCALAI_HOST").unwrap_or_else(|_| "http://localhost:8080".into());
    let agent = http_agent();

    // LocalAI exposes OpenAI-compatible /v1/models
    let server_running = agent.get(&format!("{url}/v1/models")).call().is_ok();

    // Also check for the local-ai binary
    let binary = find_binary("local-ai");

    // Check if it's running via Docker (look for localai container)
    let docker_running = Command::new("docker")
        .args([
            "ps",
            "--filter",
            "ancestor=localai/localai",
            "--format",
            "{{.ID}}",
        ])
        .output()
        .ok()
        .is_some_and(|o| o.status.success() && !o.stdout.is_empty());

    DetectedBackend {
        backend: Backend::LocalAi,
        available: server_running || binary.is_some() || docker_running,
        binary_path: binary,
        api_url: Some(url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_labels() {
        assert_eq!(Backend::LlamaServer.label(), "llama-server");
        assert_eq!(Backend::Ollama.label(), "Ollama");
        assert_eq!(Backend::MlxLm.label(), "MLX");
        assert_eq!(Backend::LmStudio.label(), "LM Studio");
        assert_eq!(Backend::Vllm.label(), "vLLM");
        assert_eq!(Backend::KoboldCpp.label(), "KoboldCpp");
        assert_eq!(Backend::LocalAi.label(), "LocalAI");
        assert_eq!(Backend::Lemonade.label(), "Lemonade");
        assert_eq!(Backend::FastFlowLm.label(), "FastFlowLM");
    }

    #[test]
    fn backend_local_serve_support() {
        use crate::models::ModelFormat;
        // Can serve local GGUF files
        assert!(Backend::LlamaServer.can_serve_local_gguf());
        assert!(Backend::KoboldCpp.can_serve_local_gguf());
        assert!(Backend::LocalAi.can_serve_local_gguf());
        // Cannot serve local GGUF files (use their own registries)
        assert!(!Backend::Ollama.can_serve_local_gguf());
        assert!(!Backend::Vllm.can_serve_local_gguf());
        assert!(Backend::Lemonade.can_serve_local_gguf());
        assert!(!Backend::FastFlowLm.can_serve_local_gguf());
        assert!(!Backend::LmStudio.can_serve_local_gguf());
        // MLX
        assert!(Backend::MlxLm.can_serve_local_mlx());
        assert!(!Backend::LlamaServer.can_serve_local_mlx());
        assert!(!Backend::LocalAi.can_serve_local_mlx());
        // Combined check
        assert!(Backend::LlamaServer.can_serve_local(&ModelFormat::Gguf));
        assert!(!Backend::LlamaServer.can_serve_local(&ModelFormat::Mlx));
        assert!(Backend::MlxLm.can_serve_local(&ModelFormat::Mlx));
        assert!(!Backend::Ollama.can_serve_local(&ModelFormat::Gguf));
        assert!(Backend::LocalAi.can_serve_local(&ModelFormat::Gguf));
        assert!(Backend::Lemonade.can_serve_local(&ModelFormat::Gguf));
        assert!(!Backend::FastFlowLm.can_serve_local(&ModelFormat::Gguf));
    }

    #[test]
    fn incompatible_backends_have_reasons() {
        assert!(Backend::Ollama.local_serve_reason().is_some());
        assert!(Backend::LmStudio.local_serve_reason().is_some());
        assert!(Backend::Vllm.local_serve_reason().is_some());
        assert!(Backend::FastFlowLm.local_serve_reason().is_some());
        assert!(Backend::LlamaServer.local_serve_reason().is_none());
        assert!(Backend::KoboldCpp.local_serve_reason().is_none());
        assert!(Backend::MlxLm.local_serve_reason().is_none());
        assert!(Backend::LocalAi.local_serve_reason().is_none());
        assert!(Backend::Lemonade.local_serve_reason().is_none());
    }

    #[test]
    fn backend_keys() {
        assert_eq!(backend_key(&Backend::LlamaServer), "llama-server");
        assert_eq!(backend_key(&Backend::Ollama), "ollama");
        assert_eq!(backend_key(&Backend::MlxLm), "mlx");
        assert_eq!(backend_key(&Backend::LmStudio), "lm-studio");
        assert_eq!(backend_key(&Backend::Vllm), "vllm");
        assert_eq!(backend_key(&Backend::KoboldCpp), "koboldcpp");
        assert_eq!(backend_key(&Backend::LocalAi), "localai");
        assert_eq!(backend_key(&Backend::Lemonade), "lemonade");
        assert_eq!(backend_key(&Backend::FastFlowLm), "fastflowlm");
    }

    #[test]
    fn detected_backend_status_labels() {
        let available = DetectedBackend {
            backend: Backend::LlamaServer,
            available: true,
            binary_path: Some("/usr/local/bin/llama-server".into()),
            api_url: None,
        };
        assert_eq!(available.status_label(), "ready");

        let unavailable = DetectedBackend {
            backend: Backend::Ollama,
            available: false,
            binary_path: None,
            api_url: None,
        };
        assert_eq!(unavailable.status_label(), "not found");
    }

    #[test]
    fn detect_backends_returns_nine() {
        let backends = detect_backends();
        assert_eq!(backends.len(), 9);
        assert_eq!(backends[0].backend, Backend::LlamaServer);
        assert_eq!(backends[1].backend, Backend::Ollama);
        assert_eq!(backends[2].backend, Backend::MlxLm);
        assert_eq!(backends[3].backend, Backend::LmStudio);
        assert_eq!(backends[4].backend, Backend::Vllm);
        assert_eq!(backends[5].backend, Backend::KoboldCpp);
        assert_eq!(backends[6].backend, Backend::LocalAi);
        assert_eq!(backends[7].backend, Backend::Lemonade);
        assert_eq!(backends[8].backend, Backend::FastFlowLm);
    }
}
