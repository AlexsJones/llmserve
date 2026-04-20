use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackendPreset {
    pub ctx_size: Option<u32>,
    pub use_ctx_size: Option<bool>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub flash_attn: Option<bool>,
    pub batch_size: Option<u32>,
    pub gpu_layers: Option<i32>,
    pub threads: Option<u32>,
    pub extra_args: Vec<String>,
}

impl Default for BackendPreset {
    fn default() -> Self {
        Self {
            ctx_size: None,
            host: None,
            port: None,
            use_ctx_size: None,
            flash_attn: None,
            batch_size: None,
            gpu_layers: None,
            threads: None,
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub extra_model_dirs: Vec<PathBuf>,
    pub preferred_port: u16,
    pub preferred_host: String,
    pub use_ctx_size: bool,
    pub default_ctx_size: u32,
    pub flash_attn: bool,
    pub default_backend: Option<String>,
    pub theme: Option<String>,
    pub presets: HashMap<String, BackendPreset>,
}

impl Default for Config {
    fn default() -> Self {
        let mut presets = HashMap::new();

        presets.insert(
            "llama-server".into(),
            BackendPreset {
                ctx_size: Some(8192),
                flash_attn: Some(true),
                batch_size: Some(2048),
                gpu_layers: Some(-1),
                ..Default::default()
            },
        );

        presets.insert(
            "ollama".into(),
            BackendPreset {
                ctx_size: Some(8192),
                ..Default::default()
            },
        );

        presets.insert(
            "mlx".into(),
            BackendPreset {
                ctx_size: Some(4096),
                ..Default::default()
            },
        );

        presets.insert("lm-studio".into(), BackendPreset::default());

        presets.insert(
            "vllm".into(),
            BackendPreset {
                ctx_size: Some(8192),
                port: Some(8000),
                gpu_layers: Some(-1),
                ..Default::default()
            },
        );

        presets.insert(
            "koboldcpp".into(),
            BackendPreset {
                ctx_size: Some(8192),
                gpu_layers: Some(-1),
                port: Some(5001),
                ..Default::default()
            },
        );

        presets.insert(
            "localai".into(),
            BackendPreset {
                ctx_size: Some(8192),
                port: Some(8080),
                ..Default::default()
            },
        );

        presets.insert(
            "lemonade".into(),
            BackendPreset {
                ctx_size: Some(4096),
                port: Some(8000),
                ..Default::default()
            },
        );

        presets.insert(
            "fastflowlm".into(),
            BackendPreset {
                port: Some(52625),
                ..Default::default()
            },
        );

        Self {
            extra_model_dirs: Vec::new(),
            preferred_port: 8080,
            preferred_host: "0.0.0.0".into(),
            use_ctx_size: true,
            default_ctx_size: 8192,
            flash_attn: true,
            default_backend: None,
            theme: None,
            presets,
        }
    }
}

impl Config {
    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("llmserve").join("config.toml"))
    }

    pub fn load() -> Self {
        Self::config_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(s) = toml::to_string_pretty(self) {
                let _ = fs::write(&path, s);
            }
        }
    }

    /// Get the preset for a backend, merging with global defaults.
    /// Backend key is the lowercase label: "llama-server", "ollama", "mlx", "lm-studio"
    pub fn preset_for(&self, backend_key: &str) -> ResolvedPreset {
        let preset = self.presets.get(backend_key);
        ResolvedPreset {
            ctx_size: preset
                .and_then(|p| p.ctx_size)
                .unwrap_or(self.default_ctx_size),
            use_ctx_size: preset
                .and_then(|p| p.use_ctx_size)
                .unwrap_or(self.use_ctx_size),
            host: preset
                .and_then(|p| p.host.clone())
                .unwrap_or_else(|| self.preferred_host.clone()),
            port: preset.and_then(|p| p.port).unwrap_or(self.preferred_port),
            flash_attn: preset.and_then(|p| p.flash_attn).unwrap_or(self.flash_attn),
            batch_size: preset.and_then(|p| p.batch_size),
            gpu_layers: preset.and_then(|p| p.gpu_layers),
            threads: preset.and_then(|p| p.threads),
            extra_args: preset.map(|p| p.extra_args.clone()).unwrap_or_default(),
        }
    }
}

/// Fully resolved preset with no Option fields — ready to use for launching.
#[derive(Debug, Clone)]
pub struct ResolvedPreset {
    pub ctx_size: u32,
    pub use_ctx_size: bool,
    pub host: String,
    pub port: u16,
    pub flash_attn: bool,
    pub batch_size: Option<u32>,
    pub gpu_layers: Option<i32>,
    pub threads: Option<u32>,
    pub extra_args: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_all_presets() {
        let config = Config::default();
        assert!(config.presets.contains_key("llama-server"));
        assert!(config.presets.contains_key("ollama"));
        assert!(config.presets.contains_key("mlx"));
        assert!(config.presets.contains_key("lm-studio"));
        assert!(config.presets.contains_key("vllm"));
        assert!(config.presets.contains_key("koboldcpp"));
        assert!(config.presets.contains_key("localai"));
        assert!(config.presets.contains_key("lemonade"));
        assert!(config.presets.contains_key("fastflowlm"));
    }

    #[test]
    fn preset_for_known_backend() {
        let config = Config::default();
        let preset = config.preset_for("llama-server");
        assert_eq!(preset.ctx_size, 8192);
        assert!(preset.flash_attn);
        assert_eq!(preset.batch_size, Some(2048));
        assert_eq!(preset.gpu_layers, Some(-1));
    }

    #[test]
    fn preset_for_unknown_backend_uses_globals() {
        let config = Config::default();
        let preset = config.preset_for("unknown-backend");
        assert_eq!(preset.ctx_size, config.default_ctx_size);
        assert_eq!(preset.host, config.preferred_host);
        assert_eq!(preset.port, config.preferred_port);
        assert_eq!(preset.flash_attn, config.flash_attn);
        assert!(preset.extra_args.is_empty());
    }

    #[test]
    fn preset_overrides_globals() {
        let mut config = Config::default();
        config.presets.insert(
            "llama-server".into(),
            BackendPreset {
                ctx_size: Some(32768),
                port: Some(9090),
                flash_attn: Some(false),
                ..Default::default()
            },
        );
        let preset = config.preset_for("llama-server");
        assert_eq!(preset.ctx_size, 32768);
        assert_eq!(preset.port, 9090);
        assert!(!preset.flash_attn);
        // host falls back to global since preset didn't set it
        assert_eq!(preset.host, config.preferred_host);
    }

    #[test]
    fn config_roundtrip_toml() {
        let config = Config::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.preferred_port, config.preferred_port);
        assert_eq!(deserialized.default_ctx_size, config.default_ctx_size);
        assert_eq!(deserialized.presets.len(), config.presets.len());
    }

    #[test]
    fn config_partial_toml_uses_defaults() {
        let toml_str = r#"
            preferred_port = 9999
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.preferred_port, 9999);
        assert_eq!(config.preferred_host, "0.0.0.0");
        assert_eq!(config.default_ctx_size, 8192);
        // presets should be empty since we didn't specify them and #[serde(default)] fills Default
        // Actually with serde(default), missing fields get Default values
    }

    #[test]
    fn config_custom_preset_from_toml() {
        let toml_str = r#"
            preferred_port = 8080

            [presets.llama-server]
            ctx_size = 65536
            flash_attn = true
            batch_size = 512
            gpu_layers = 99
            threads = 8
            extra_args = ["--mlock", "--cont-batching"]
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let preset = config.preset_for("llama-server");
        assert_eq!(preset.ctx_size, 65536);
        assert_eq!(preset.batch_size, Some(512));
        assert_eq!(preset.gpu_layers, Some(99));
        assert_eq!(preset.threads, Some(8));
        assert_eq!(preset.extra_args, vec!["--mlock", "--cont-batching"]);
    }
}
