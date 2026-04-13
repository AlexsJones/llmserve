use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelFormat {
    Gguf,
    Mlx,
}

impl fmt::Display for ModelFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelFormat::Gguf => write!(f, "GGUF"),
            ModelFormat::Mlx => write!(f, "MLX"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSource {
    LmStudio,
    LlamaCppCache,
    HfCache,
    Ollama,
    Lemonade,
    FastFlowLm,
    LlmFit,
    ExtraDir,
}

impl fmt::Display for ModelSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelSource::LmStudio => write!(f, "LM Studio"),
            ModelSource::LlamaCppCache => write!(f, "llama.cpp"),
            ModelSource::HfCache => write!(f, "HF Cache"),
            ModelSource::Ollama => write!(f, "Ollama"),
            ModelSource::Lemonade => write!(f, "Lemonade"),
            ModelSource::FastFlowLm => write!(f, "FastFlowLM"),
            ModelSource::LlmFit => write!(f, "llmfit"),
            ModelSource::ExtraDir => write!(f, "Custom"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveredModel {
    pub name: String,
    pub path: PathBuf,
    pub mmproj: Option<PathBuf>,
    pub format: ModelFormat,
    pub size_bytes: u64,
    pub quant: Option<String>,
    pub param_hint: Option<String>,
    pub source: ModelSource,
}

impl DiscoveredModel {
    pub fn size_display(&self) -> String {
        if self.size_bytes == 0 {
            return "-".into();
        }
        let gb = self.size_bytes as f64 / 1_073_741_824.0;
        if gb >= 1.0 {
            format!("{:.1}G", gb)
        } else {
            let mb = self.size_bytes as f64 / 1_048_576.0;
            format!("{:.0}M", mb)
        }
    }
}

pub fn discover_models(extra_dirs: &[PathBuf]) -> Vec<DiscoveredModel> {
    let mut models = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    let home = dirs::home_dir().unwrap_or_default();

    // LM Studio models — differs by platform
    let lmstudio_dir = if cfg!(windows) {
        // Windows: %USERPROFILE%/.lmstudio/models (newer) or %LOCALAPPDATA%/LM Studio/models
        let primary = home.join(".lmstudio").join("models");
        if primary.is_dir() {
            primary
        } else {
            dirs::data_local_dir()
                .unwrap_or_else(|| home.clone())
                .join("LM Studio")
                .join("models")
        }
    } else {
        home.join(".lmstudio").join("models")
    };
    scan_gguf_dir(
        &lmstudio_dir,
        ModelSource::LmStudio,
        &mut models,
        &mut seen_paths,
    );

    // LM Studio newer versions may use ~/.cache/lm-studio/models/
    let lmstudio_cache_dir = home.join(".cache").join("lm-studio").join("models");
    if lmstudio_cache_dir.is_dir() && lmstudio_cache_dir != lmstudio_dir {
        scan_gguf_dir(
            &lmstudio_cache_dir,
            ModelSource::LmStudio,
            &mut models,
            &mut seen_paths,
        );
    }

    // llama.cpp cache — on Windows use %LOCALAPPDATA%/llm-models as fallback
    let llamacpp_dir = if cfg!(windows) {
        let cache = home.join(".cache").join("llm-models");
        if cache.is_dir() {
            cache
        } else {
            dirs::data_local_dir()
                .unwrap_or_else(|| home.clone())
                .join("llm-models")
        }
    } else {
        home.join(".cache").join("llm-models")
    };
    scan_gguf_dir(
        &llamacpp_dir,
        ModelSource::LlamaCppCache,
        &mut models,
        &mut seen_paths,
    );

    // llmfit cache
    let llmfit_dir = home.join(".cache").join("llmfit").join("models");
    scan_gguf_dir(
        &llmfit_dir,
        ModelSource::LlmFit,
        &mut models,
        &mut seen_paths,
    );

    // HuggingFace cache — MLX models
    let hf_hub = std::env::var("HF_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if cfg!(windows) {
                home.join(".cache").join("huggingface").join("hub")
            } else {
                home.join(".cache").join("huggingface").join("hub")
            }
        });
    scan_mlx_models(&hf_hub, &mut models, &mut seen_paths);

    // Extra user-configured directories
    for dir in extra_dirs {
        scan_gguf_dir(dir, ModelSource::ExtraDir, &mut models, &mut seen_paths);
    }

    models.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    models
}

fn scan_gguf_dir(
    dir: &Path,
    source: ModelSource,
    models: &mut Vec<DiscoveredModel>,
    seen: &mut std::collections::HashSet<PathBuf>,
) {
    if !dir.is_dir() {
        return;
    }

    for entry in WalkDir::new(dir).min_depth(1).into_iter().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "gguf") {
            let fname = path.file_name().unwrap().to_string_lossy();
            if fname.starts_with("mmproj") {
                continue;
            }
            if !seen.insert(path.to_path_buf()) {
                continue;
            }

            let parent = path.parent().unwrap();
            let mmproj = find_mmproj(parent);
            let size_bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            let dir_name = parent.file_name().unwrap().to_string_lossy().to_string();

            // If the file lives directly in the scan root (flat layout), the
            // parent dir name is not model-specific — fall back to the file
            // stem so each model gets a distinct, informative name.
            let file_stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| fname.to_string());
            let name = if parent == dir {
                file_stem.clone()
            } else {
                dir_name.clone()
            };
            let param_source = if parent == dir { &file_stem } else { &dir_name };

            models.push(DiscoveredModel {
                name,
                path: path.to_path_buf(),
                mmproj,
                format: ModelFormat::Gguf,
                size_bytes,
                quant: parse_quant(&fname),
                param_hint: parse_params(param_source),
                source: source.clone(),
            });
        }
    }
}

fn scan_mlx_models(
    hf_hub: &Path,
    models: &mut Vec<DiscoveredModel>,
    seen: &mut std::collections::HashSet<PathBuf>,
) {
    if !hf_hub.is_dir() {
        return;
    }

    // HF cache structure: models--<owner>--<repo>/snapshots/<hash>/
    let Ok(entries) = fs::read_dir(hf_hub) else {
        return;
    };
    for entry in entries.flatten() {
        let dir_name = entry.file_name().to_string_lossy().to_string();
        if !dir_name.starts_with("models--") {
            continue;
        }
        // Check if it's an MLX model (owner is mlx-community or name contains mlx)
        let lower = dir_name.to_lowercase();
        if !lower.contains("mlx") {
            continue;
        }

        let snapshots_dir = entry.path().join("snapshots");
        if !snapshots_dir.is_dir() {
            continue;
        }

        // Find the latest snapshot (there's usually just one)
        let Some(snapshot) = fs::read_dir(&snapshots_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        else {
            continue;
        };

        let snap_path = snapshot.path();

        // Verify it has config.json and safetensors
        if !snap_path.join("config.json").exists() {
            continue;
        }
        let has_safetensors = fs::read_dir(&snap_path)
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| e.path().extension().is_some_and(|ext| ext == "safetensors"));
        if !has_safetensors {
            continue;
        }

        if !seen.insert(snap_path.clone()) {
            continue;
        }

        // Calculate total size of safetensors files
        let size_bytes: u64 = fs::read_dir(&snap_path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "safetensors"))
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();

        // Parse friendly name from "models--mlx-community--Qwen3.5-9B-4bit"
        let friendly = dir_name
            .strip_prefix("models--")
            .unwrap_or(&dir_name)
            .replace("--", "/");

        let quant = if lower.contains("8bit") || lower.contains("8-bit") {
            Some("8bit".into())
        } else if lower.contains("4bit") || lower.contains("4-bit") {
            Some("4bit".into())
        } else {
            None
        };

        models.push(DiscoveredModel {
            name: friendly.clone(),
            path: snap_path,
            mmproj: None,
            format: ModelFormat::Mlx,
            size_bytes,
            quant,
            param_hint: parse_params(&friendly),
            source: ModelSource::HfCache,
        });
    }
}

/// Normalize a model name for fuzzy dedup: lowercase, strip org prefixes,
/// strip trailing `-gguf`, and unify separators.
fn normalize_model_name(name: &str) -> String {
    let lower = name.to_lowercase();
    // Strip org prefix like "nvidia/" or "qwen/"
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    // Strip common suffixes
    let base = base
        .strip_suffix("-gguf")
        .or_else(|| base.strip_suffix(".gguf"))
        .unwrap_or(base);
    // Unify separators
    base.replace('_', "-")
}

pub fn add_lmstudio_models(models: &mut Vec<DiscoveredModel>, api_models: Vec<(String, u64)>) {
    let existing_normalized: Vec<String> = models
        .iter()
        .filter(|m| m.source == ModelSource::LmStudio)
        .map(|m| normalize_model_name(&m.name))
        .collect();

    for (id, size) in api_models {
        // Skip if we already discovered this model from disk.
        // Names differ between API (e.g. "nvidia/nemotron-3-nano-4b") and
        // disk (e.g. "NVIDIA-Nemotron-3-Nano-4B-GGUF"), so normalize both.
        let api_norm = normalize_model_name(&id);
        if existing_normalized
            .iter()
            .any(|e| e.contains(&api_norm) || api_norm.contains(e.as_str()))
        {
            continue;
        }
        models.push(DiscoveredModel {
            name: id.clone(),
            path: PathBuf::from(format!("lmstudio:{id}")),
            mmproj: None,
            format: ModelFormat::Gguf,
            size_bytes: size,
            quant: parse_quant(&id),
            param_hint: parse_params(&id),
            source: ModelSource::LmStudio,
        });
    }
    models.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
}

pub fn add_lemonade_models(models: &mut Vec<DiscoveredModel>, api_models: Vec<(String, u64)>) {
    for (id, size) in api_models {
        models.push(DiscoveredModel {
            name: id.clone(),
            path: PathBuf::from(format!("lemonade:{id}")),
            mmproj: None,
            format: ModelFormat::Gguf,
            size_bytes: size,
            quant: parse_quant(&id),
            param_hint: parse_params(&id),
            source: ModelSource::Lemonade,
        });
    }
    models.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
}

pub fn add_fastflowlm_models(models: &mut Vec<DiscoveredModel>, api_models: Vec<(String, u64)>) {
    for (id, size) in api_models {
        models.push(DiscoveredModel {
            name: id.clone(),
            path: PathBuf::from(format!("fastflowlm:{id}")),
            mmproj: None,
            format: ModelFormat::Gguf,
            size_bytes: size,
            quant: parse_quant(&id),
            param_hint: parse_params(&id),
            source: ModelSource::FastFlowLm,
        });
    }
    models.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
}

pub fn add_ollama_models(models: &mut Vec<DiscoveredModel>, ollama_models: Vec<(String, u64)>) {
    for (name, size) in ollama_models {
        models.push(DiscoveredModel {
            name: name.clone(),
            path: PathBuf::from(format!("ollama:{name}")),
            mmproj: None,
            format: ModelFormat::Gguf,
            size_bytes: size,
            quant: parse_quant(&name),
            param_hint: parse_params(&name),
            source: ModelSource::Ollama,
        });
    }
    models.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
}

fn find_mmproj(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .find(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("mmproj") && n.ends_with(".gguf")
        })
        .map(|e| e.path())
}

fn parse_quant(s: &str) -> Option<String> {
    // Match patterns like Q4_K_M, Q8_0, IQ4_NL, Q5_K_S, etc.
    let upper = s.to_uppercase();
    for part in upper.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if part.starts_with('Q')
            && part.len() >= 3
            && part.chars().nth(1).is_some_and(|c| c.is_ascii_digit())
        {
            return Some(part.to_string());
        }
        if part.starts_with("IQ")
            && part.len() >= 4
            && part.chars().nth(2).is_some_and(|c| c.is_ascii_digit())
        {
            return Some(part.to_string());
        }
    }
    // Check for "4bit" / "8bit" style
    for part in s.split(|c: char| !c.is_alphanumeric()) {
        let lower = part.to_lowercase();
        if lower == "4bit" || lower == "8bit" || lower == "fp16" || lower == "bf16" {
            return Some(lower);
        }
    }
    None
}

fn parse_params(s: &str) -> Option<String> {
    // Match patterns like "27B", "3.5B", "35B-A3B", "4B"
    let upper = s.to_uppercase();
    // First try MoE pattern like "35B-A3B"
    for window in upper
        .split(|c: char| c == '-' || c == '_')
        .collect::<Vec<_>>()
        .windows(2)
    {
        if let [total, active] = window {
            if total.ends_with('B')
                && active.starts_with('A')
                && active.ends_with('B')
                && total[..total.len() - 1].parse::<f64>().is_ok()
            {
                return Some(format!("{total}-{active}"));
            }
        }
    }
    // Then simple "NB" pattern
    for part in upper.split(|c: char| c == '-' || c == '_' || c == ' ') {
        if part.ends_with('B') && part.len() >= 2 {
            let num_part = &part[..part.len() - 1];
            if num_part.parse::<f64>().is_ok() {
                return Some(part.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quant_q4_k_m() {
        assert_eq!(parse_quant("Qwen3.5-9B-Q4_K_M.gguf"), Some("Q4_K_M".into()));
    }

    #[test]
    fn parse_quant_q8_0() {
        assert_eq!(parse_quant("model-Q8_0.gguf"), Some("Q8_0".into()));
    }

    #[test]
    fn parse_quant_iq4_nl() {
        assert_eq!(parse_quant("model-IQ4_NL.gguf"), Some("IQ4_NL".into()));
    }

    #[test]
    fn parse_quant_4bit() {
        assert_eq!(parse_quant("model-4bit"), Some("4bit".into()));
    }

    #[test]
    fn parse_quant_fp16() {
        assert_eq!(parse_quant("model-fp16.gguf"), Some("fp16".into()));
    }

    #[test]
    fn parse_quant_none() {
        assert_eq!(parse_quant("just-a-model-name"), None);
    }

    #[test]
    fn parse_params_simple() {
        assert_eq!(parse_params("Qwen3.5-9B-Instruct"), Some("9B".into()));
    }

    #[test]
    fn parse_params_large() {
        assert_eq!(parse_params("Qwen3.5-27B-Claude"), Some("27B".into()));
    }

    #[test]
    fn parse_params_small() {
        assert_eq!(parse_params("NVIDIA-Nemotron-3-Nano-4B"), Some("4B".into()));
    }

    #[test]
    fn parse_params_moe() {
        assert_eq!(parse_params("Qwen3.5-35B-A3B-GGUF"), Some("35B-A3B".into()));
    }

    #[test]
    fn parse_params_decimal() {
        assert_eq!(parse_params("Model-3.5B-Instruct"), Some("3.5B".into()));
    }

    #[test]
    fn parse_params_none() {
        assert_eq!(parse_params("some-model-name"), None);
    }

    #[test]
    fn size_display_gigabytes() {
        let model = DiscoveredModel {
            name: "test".into(),
            path: PathBuf::from("test.gguf"),
            mmproj: None,
            format: ModelFormat::Gguf,
            size_bytes: 5_368_709_120, // 5 GB
            quant: None,
            param_hint: None,
            source: ModelSource::ExtraDir,
        };
        assert_eq!(model.size_display(), "5.0G");
    }

    #[test]
    fn size_display_megabytes() {
        let model = DiscoveredModel {
            name: "test".into(),
            path: PathBuf::from("test.gguf"),
            mmproj: None,
            format: ModelFormat::Gguf,
            size_bytes: 524_288_000, // 500 MB
            quant: None,
            param_hint: None,
            source: ModelSource::ExtraDir,
        };
        assert_eq!(model.size_display(), "500M");
    }

    #[test]
    fn model_format_display() {
        assert_eq!(ModelFormat::Gguf.to_string(), "GGUF");
        assert_eq!(ModelFormat::Mlx.to_string(), "MLX");
    }

    #[test]
    fn model_source_display() {
        assert_eq!(ModelSource::LmStudio.to_string(), "LM Studio");
        assert_eq!(ModelSource::LlamaCppCache.to_string(), "llama.cpp");
        assert_eq!(ModelSource::HfCache.to_string(), "HF Cache");
        assert_eq!(ModelSource::Ollama.to_string(), "Ollama");
        assert_eq!(ModelSource::Lemonade.to_string(), "Lemonade");
        assert_eq!(ModelSource::FastFlowLm.to_string(), "FastFlowLM");
        assert_eq!(ModelSource::LlmFit.to_string(), "llmfit");
        assert_eq!(ModelSource::ExtraDir.to_string(), "Custom");
    }

    #[test]
    fn add_ollama_models_sorts() {
        let mut models = vec![DiscoveredModel {
            name: "Zebra-Model".into(),
            path: PathBuf::from("z.gguf"),
            mmproj: None,
            format: ModelFormat::Gguf,
            size_bytes: 100,
            quant: None,
            param_hint: None,
            source: ModelSource::ExtraDir,
        }];
        add_ollama_models(&mut models, vec![("alpha-model".into(), 200)]);
        assert_eq!(models[0].name, "alpha-model");
        assert_eq!(models[1].name, "Zebra-Model");
    }

    #[test]
    fn discover_models_with_empty_extra_dirs() {
        let models = discover_models(&[PathBuf::from("/nonexistent/path/12345")]);
        let _ = models.len();
    }

    #[test]
    fn scan_gguf_dir_flat_layout_uses_file_stem() {
        // Simulate a flat user directory with multiple .gguf files (e.g. a
        // bunch of symlinks) — each model should get a distinct name derived
        // from the file stem, not the shared parent dir name.
        let tmp = std::env::temp_dir().join(format!("llmserve_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("Qwen3-8B-Q8_0.gguf"), b"x").unwrap();
        fs::write(tmp.join("qwen2.5-coder-3b-q8_0.gguf"), b"x").unwrap();

        let mut models = Vec::new();
        let mut seen = std::collections::HashSet::new();
        scan_gguf_dir(&tmp, ModelSource::ExtraDir, &mut models, &mut seen);

        let names: std::collections::HashSet<String> =
            models.iter().map(|m| m.name.clone()).collect();
        assert!(names.contains("Qwen3-8B-Q8_0"), "got names: {names:?}");
        assert!(
            names.contains("qwen2.5-coder-3b-q8_0"),
            "got names: {names:?}"
        );
        assert_eq!(names.len(), 2, "names should be distinct, got: {names:?}");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn scan_gguf_dir_nested_layout_uses_dir_name() {
        // LM Studio / HF-style nested layout: parent dir name is the model name.
        let tmp = std::env::temp_dir().join(format!("llmserve_test_nested_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let subdir = tmp.join("Qwen3-8B-GGUF");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(subdir.join("model-Q8_0.gguf"), b"x").unwrap();

        let mut models = Vec::new();
        let mut seen = std::collections::HashSet::new();
        scan_gguf_dir(&tmp, ModelSource::LmStudio, &mut models, &mut seen);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Qwen3-8B-GGUF");

        fs::remove_dir_all(&tmp).unwrap();
    }
}
