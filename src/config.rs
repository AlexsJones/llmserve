use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// `config.toml` exists but could not be read or parsed.
/// The file on disk is left unchanged.
#[derive(Debug)]
pub struct ConfigLoadError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for ConfigLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "could not load config {}: {}\nllmserve left the file unchanged. Fix it, or move it aside, then start again.",
            self.path.display(),
            self.message
        )
    }
}

impl std::error::Error for ConfigLoadError {}

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

    /// Load the user config.
    ///
    /// A missing file is a first run: the defaults are returned and a later
    /// [`save`](Self::save) may create the file. A file that cannot be read
    /// or parsed is an error. Callers must not replace that file with defaults.
    pub fn load() -> Result<Self, ConfigLoadError> {
        match Self::config_path() {
            Some(path) => Self::load_at(&path),
            None => Ok(Self::default()),
        }
    }

    /// Load `path`, or the defaults when `path` does not exist.
    pub fn load_at(path: &Path) -> Result<Self, ConfigLoadError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path).map_err(|e| ConfigLoadError {
            path: path.to_path_buf(),
            message: format!("could not read file: {e}"),
        })?;
        toml::from_str(&text).map_err(|e| ConfigLoadError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    /// Write this config to the user config path.
    ///
    /// Refuses to replace a file that is already there and does not parse, so
    /// a bad config loaded as defaults can never be saved back over the user's
    /// file.
    pub fn save(&self) -> Result<(), String> {
        match Self::config_path() {
            Some(path) => self.save_at(&path),
            None => Err("could not determine a config directory".into()),
        }
    }

    /// Write this config to `path`, creating parent directories when needed.
    ///
    /// See [`save`](Self::save) for the refusal to overwrite an unparseable file.
    pub fn save_at(&self, path: &Path) -> Result<(), String> {
        if path.exists() {
            match fs::read_to_string(path) {
                Ok(text) => {
                    if let Err(e) = toml::from_str::<Config>(&text) {
                        return Err(format!(
                            "left {} unchanged because it does not parse: {e}",
                            path.display()
                        ));
                    }
                }
                Err(e) => {
                    return Err(format!(
                        "left {} unchanged because it could not be read: {e}",
                        path.display()
                    ));
                }
            }
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!(
                        "could not create config directory {}: {e}",
                        parent.display()
                    )
                })?;
            }
        }
        let text =
            toml::to_string_pretty(self).map_err(|e| format!("could not serialize config: {e}"))?;
        fs::write(path, text).map_err(|e| format!("could not write {}: {e}", path.display()))
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

    struct Tmp {
        path: PathBuf,
    }

    impl Tmp {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "llmserve-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn file(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn missing_config_uses_defaults_and_save_creates_it() {
        let tmp = Tmp::new("missing");
        let path = tmp.file("config.toml");
        let loaded = Config::load_at(&path).unwrap();
        assert_eq!(loaded.preferred_port, Config::default().preferred_port);

        let mut config = loaded;
        config.preferred_port = 9091;
        config.save_at(&path).unwrap();

        let again = Config::load_at(&path).unwrap();
        assert_eq!(again.preferred_port, 9091);
    }

    #[test]
    fn invalid_config_is_reported_and_not_overwritten() {
        let tmp = Tmp::new("invalid");
        let path = tmp.file("config.toml");
        // A type error is valid-looking TOML that `Config` cannot accept.
        // This is the case `toml::from_str().ok()` used to swallow.
        let original = "\
# user settings — do not replace this file
preferred_port = \"not-a-number\"
extra_model_dirs = [\"/models/kept\"]
";
        fs::write(&path, original).unwrap();

        let err = Config::load_at(&path).expect_err("bad toml must not become defaults");
        let message = err.to_string();
        assert!(
            message.contains(&path.display().to_string()),
            "error should name the file: {message}"
        );
        assert!(
            message.contains("not-a-number") || message.contains("preferred_port"),
            "error should describe the parse failure: {message}"
        );

        // The old bug: load fell back to Default, and the next save clobbered the file.
        let save_err = Config::default()
            .save_at(&path)
            .expect_err("save must refuse");
        assert!(
            save_err.contains("unchanged"),
            "refusal should say the file was left alone: {save_err}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn unreadable_config_is_reported_and_not_overwritten() {
        let tmp = Tmp::new("unreadable");
        // A directory where the file should be cannot be read as TOML.
        let path = tmp.file("config.toml");
        fs::create_dir_all(&path).unwrap();

        let err = Config::load_at(&path).expect_err("unreadable path must not become defaults");
        assert!(err.to_string().contains(&path.display().to_string()));

        let save_err = Config::default()
            .save_at(&path)
            .expect_err("save must refuse");
        assert!(save_err.contains("unchanged"), "{save_err}");
        assert!(path.is_dir(), "save must not replace the path");
    }
}
