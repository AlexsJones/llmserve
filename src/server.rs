use crate::backends::Backend;
use crate::config::{Config, ResolvedPreset};
use crate::models::DiscoveredModel;
use std::collections::VecDeque;
use std::io::Read;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const MAX_LOG_LINES: usize = 1000;

/// A chunk of child output forwarded from a reader thread.
enum LogMsg {
    /// A complete, newline-terminated line.
    Line(String),
    /// A carriage-return progress update that should replace the previous one.
    Progress(String),
}

pub struct ServerHandle {
    pub backend: Backend,
    pub model_name: String,
    pub pid: u32,
    pub port: u16,
    pub host: String,
    pub child: Child,
    pub started_at: Instant,
    /// Whether the server has started accepting TCP connections.
    pub ready: bool,
    /// The model and preset used to launch, kept so a crashed server can be
    /// relaunched with identical settings.
    pub model: DiscoveredModel,
    pub preset: ResolvedPreset,
    /// Ring buffer of log lines (combined stdout + stderr).
    pub log_lines: VecDeque<String>,
    rx: Receiver<LogMsg>,
    readers: Vec<JoinHandle<()>>,
    last_probe: Option<Instant>,
    last_was_progress: bool,
}

impl ServerHandle {
    pub fn uptime_display(&self) -> String {
        let secs = self.started_at.elapsed().as_secs();
        if secs < 60 {
            format!("{secs}s")
        } else if secs < 3600 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else {
            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
        }
    }

    pub fn display_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// Pull any output forwarded by the reader threads into the ring buffer.
    pub fn drain_output(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.apply_log(msg);
        }
    }

    fn apply_log(&mut self, msg: LogMsg) {
        let (text, is_progress) = match msg {
            LogMsg::Line(l) => (l, false),
            LogMsg::Progress(p) => (p, true),
        };
        // A progress update — or the final line completing one — replaces the
        // previous progress line instead of appending.
        if self.last_was_progress {
            if let Some(back) = self.log_lines.back_mut() {
                *back = text;
            } else {
                self.log_lines.push_back(text);
            }
        } else {
            self.log_lines.push_back(text);
        }
        self.last_was_progress = is_progress;
        while self.log_lines.len() > MAX_LOG_LINES {
            self.log_lines.pop_front();
        }
    }

    /// Probe whether the server is ready to answer requests (at most once per
    /// second, and never again once it succeeds). Uses HTTP /health rather
    /// than a bare TCP connect: llama-server opens its listener before the
    /// model finishes loading and answers 503 until it's actually ready.
    /// Backends without a /health endpoint answer 404, which still proves
    /// the server is up.
    pub fn probe_ready(&mut self) {
        if self.ready {
            return;
        }
        let due = self
            .last_probe
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(1));
        if !due {
            return;
        }
        self.last_probe = Some(Instant::now());
        let host = if self.host == "0.0.0.0" {
            "127.0.0.1"
        } else {
            self.host.as_str()
        };
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_millis(150)))
            .build()
            .new_agent();
        match agent
            .get(format!("http://{host}:{}/health", self.port))
            .call()
        {
            Ok(_) => self.ready = true,
            // Any HTTP answer means the server is up; 503 specifically means
            // "still loading" (llama-server semantics), so keep waiting.
            Err(ureq::Error::StatusCode(code)) if code != 503 => self.ready = true,
            Err(_) => {}
        }
    }

    /// Wait for the reader threads to finish flushing (call after the child
    /// has exited or been killed, so they see EOF promptly).
    fn join_readers(&mut self) {
        for h in self.readers.drain(..) {
            let _ = h.join();
        }
    }
}

/// Read a child pipe on a dedicated thread, forwarding complete lines and
/// carriage-return progress updates. Blocking reads are fine here — the
/// thread exits on EOF when the child dies. This is portable (no fcntl),
/// so serving works on Windows too.
fn spawn_reader<R: Read + Send + 'static>(mut src: R, tx: Sender<LogMsg>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut partial = String::new();
        let mut buf = [0u8; 4096];
        loop {
            match src.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    partial.push_str(&String::from_utf8_lossy(&buf[..n]));

                    while let Some(pos) = partial.find('\n') {
                        let line: String = partial.drain(..=pos).collect();
                        let line = line.trim_end_matches('\n').trim_end_matches('\r');
                        // Keep only the final segment of any in-line \r updates
                        let line = line.rsplit('\r').next().unwrap_or(line);
                        if tx.send(LogMsg::Line(line.to_string())).is_err() {
                            return;
                        }
                    }

                    if partial.contains('\r') {
                        let last = partial.rsplit('\r').next().unwrap_or("").to_string();
                        if !last.is_empty() && tx.send(LogMsg::Progress(last.clone())).is_err() {
                            return;
                        }
                        partial.clear();
                        partial.push_str(&last);
                    }
                }
            }
        }
        let rest = partial.trim();
        if !rest.is_empty() {
            let _ = tx.send(LogMsg::Line(rest.to_string()));
        }
    })
}

/// Check whether a port can be bound on the given host (i.e. nothing else —
/// including processes outside llmserve — is already listening on it).
pub fn port_is_free(host: &str, port: u16) -> bool {
    TcpListener::bind((host, port)).is_ok()
}

fn make_handle(
    backend: Backend,
    model: &DiscoveredModel,
    preset: &ResolvedPreset,
    mut child: Child,
) -> ServerHandle {
    let (tx, rx) = channel();
    let mut readers = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        readers.push(spawn_reader(stderr, tx.clone()));
    }
    if let Some(stdout) = child.stdout.take() {
        readers.push(spawn_reader(stdout, tx));
    }

    let pid = child.id();
    ServerHandle {
        backend,
        model_name: model.name.clone(),
        pid,
        port: preset.port,
        host: preset.host.clone(),
        child,
        started_at: Instant::now(),
        ready: false,
        model: model.clone(),
        preset: preset.clone(),
        log_lines: VecDeque::new(),
        rx,
        readers,
        last_probe: None,
        last_was_progress: false,
    }
}

pub fn launch(
    model: &DiscoveredModel,
    backend: &Backend,
    config: &Config,
) -> Result<ServerHandle, String> {
    let key = crate::backends::backend_key(backend);
    launch_with(model, backend, &config.preset_for(key))
}

/// Launch with an explicit resolved preset (e.g. edited in the confirm modal).
pub fn launch_with(
    model: &DiscoveredModel,
    backend: &Backend,
    preset: &ResolvedPreset,
) -> Result<ServerHandle, String> {
    // Check compatibility: can this backend serve this local model file?
    if !backend.can_serve_local(&model.format) {
        let reason = backend
            .local_serve_reason()
            .unwrap_or("incompatible format");
        return Err(format!(
            "{} cannot serve local {} files: {}",
            backend.label(),
            model.format,
            reason
        ));
    }

    match backend {
        Backend::LlamaServer => launch_llama_server(model, preset),
        Backend::MlxLm => launch_mlx(model, preset),
        Backend::KoboldCpp => launch_koboldcpp(model, preset),
        Backend::LocalAi => launch_localai(model, preset),
        Backend::Lemonade => launch_lemonade(model, preset),
        // These are blocked by the can_serve_local check above,
        // but match exhaustively for safety.
        Backend::Ollama | Backend::LmStudio | Backend::Vllm | Backend::FastFlowLm => Err(format!(
            "{} cannot serve local model files",
            backend.label()
        )),
    }
}

/// Newer llama.cpp (≥ b6325) takes `--flash-attn <on|off|auto>`; older builds
/// (e.g. Fedora's b6153) take it as a bare boolean flag and exit with
/// "invalid argument: on" otherwise. Probe once: with the new syntax
/// `--version` parses and exits 0, with the old syntax arg parsing fails first.
fn llama_flash_attn_takes_value() -> bool {
    static TAKES_VALUE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TAKES_VALUE.get_or_init(|| {
        Command::new("llama-server")
            .args(["--flash-attn", "on", "--version"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(true)
    })
}

fn launch_llama_server(
    model: &DiscoveredModel,
    preset: &ResolvedPreset,
) -> Result<ServerHandle, String> {
    let mut cmd = Command::new("llama-server");
    cmd.arg("--model")
        .arg(&model.path)
        .arg("--host")
        .arg(&preset.host)
        .arg("--port")
        .arg(preset.port.to_string());

    if preset.use_ctx_size {
        cmd.arg("--ctx-size").arg(preset.ctx_size.to_string());
    }

    if preset.flash_attn {
        if llama_flash_attn_takes_value() {
            cmd.arg("--flash-attn").arg("on");
        } else {
            cmd.arg("--flash-attn");
        }
    }

    if let Some(batch_size) = preset.batch_size {
        cmd.arg("--batch-size").arg(batch_size.to_string());
    } else if is_large_model(model) {
        cmd.arg("--batch-size").arg("512");
    }

    if let Some(gpu_layers) = preset.gpu_layers {
        cmd.arg("-ngl").arg(gpu_layers.to_string());
    }

    if let Some(threads) = preset.threads {
        cmd.arg("--threads").arg(threads.to_string());
    }

    if let Some(ref mmproj) = model.mmproj {
        cmd.arg("--mmproj").arg(mmproj);
    }

    for arg in &preset.extra_args {
        cmd.arg(arg);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start llama-server: {e}"))?;

    Ok(make_handle(Backend::LlamaServer, model, preset, child))
}

fn launch_mlx(model: &DiscoveredModel, preset: &ResolvedPreset) -> Result<ServerHandle, String> {
    let mut cmd = Command::new("python3");
    cmd.arg("-m")
        .arg("mlx_lm.server")
        .arg("--model")
        .arg(&model.path)
        .arg("--port")
        .arg(preset.port.to_string());

    for arg in &preset.extra_args {
        cmd.arg(arg);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start mlx_lm.server: {e}"))?;

    Ok(make_handle(Backend::MlxLm, model, preset, child))
}

fn launch_koboldcpp(
    model: &DiscoveredModel,
    preset: &ResolvedPreset,
) -> Result<ServerHandle, String> {
    let mut cmd = Command::new("koboldcpp");
    cmd.arg("--model")
        .arg(&model.path)
        .arg("--host")
        .arg(&preset.host)
        .arg("--port")
        .arg(preset.port.to_string());

    if preset.use_ctx_size {
        cmd.arg("--contextsize").arg(preset.ctx_size.to_string());
    }

    if let Some(gpu_layers) = preset.gpu_layers {
        cmd.arg("--gpulayers").arg(gpu_layers.to_string());
    }

    if let Some(threads) = preset.threads {
        cmd.arg("--threads").arg(threads.to_string());
    }

    for arg in &preset.extra_args {
        cmd.arg(arg);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start koboldcpp: {e}"))?;

    Ok(make_handle(Backend::KoboldCpp, model, preset, child))
}

fn launch_localai(
    model: &DiscoveredModel,
    preset: &ResolvedPreset,
) -> Result<ServerHandle, String> {
    // LocalAI serves models from a directory. We point --models-path at the
    // parent directory of the GGUF file so it discovers it automatically.
    let models_dir = model
        .path
        .parent()
        .ok_or_else(|| "Cannot determine model directory".to_string())?;

    let mut cmd = Command::new("local-ai");
    cmd.arg("run")
        .arg("--models-path")
        .arg(models_dir)
        .arg("--address")
        .arg(format!("{}:{}", preset.host, preset.port));

    if preset.use_ctx_size {
        cmd.arg("--context-size").arg(preset.ctx_size.to_string());
    }

    if let Some(threads) = preset.threads {
        cmd.arg("--threads").arg(threads.to_string());
    }

    for arg in &preset.extra_args {
        cmd.arg(arg);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start local-ai: {e}"))?;

    Ok(make_handle(Backend::LocalAi, model, preset, child))
}

fn launch_lemonade(
    model: &DiscoveredModel,
    preset: &ResolvedPreset,
) -> Result<ServerHandle, String> {
    let mut cmd = Command::new("lemonade");
    cmd.arg("load")
        .arg(model.path.to_string_lossy().as_ref())
        .arg("--host")
        .arg(&preset.host)
        .arg("--port")
        .arg(preset.port.to_string());

    if preset.use_ctx_size {
        cmd.arg("--ctx-size").arg(preset.ctx_size.to_string());
    }

    for arg in &preset.extra_args {
        cmd.arg(arg);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start lemonade: {e}"))?;

    Ok(make_handle(Backend::Lemonade, model, preset, child))
}

pub fn stop(handle: &mut ServerHandle) {
    // Ask nicely first so the backend can release its port and clean up,
    // then force-kill if it hasn't exited within the grace period.
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(handle.pid as libc::pid_t, libc::SIGTERM);
        }
        for _ in 0..20 {
            if matches!(handle.child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let _ = handle.child.kill();
    let _ = handle.child.wait();
    handle.join_readers();
    handle.drain_output();
}

pub fn check_exited(handle: &mut ServerHandle) -> Option<String> {
    match handle.child.try_wait() {
        Ok(Some(status)) => {
            // The pipes are closed now; wait for the readers to flush the
            // final output so the exit popup shows the actual error.
            handle.join_readers();
            handle.drain_output();
            if status.success() {
                Some("Server exited normally".into())
            } else {
                Some(format!("Server exited with status: {status}"))
            }
        }
        Ok(None) => None,
        Err(e) => Some(format!("Error checking server status: {e}")),
    }
}

fn is_large_model(model: &DiscoveredModel) -> bool {
    if let Some(ref hint) = model.param_hint {
        let upper = hint.to_uppercase();
        for part in upper.split(|c: char| c == '-' || c == '_') {
            if part.ends_with('B') {
                if let Ok(n) = part[..part.len() - 1].parse::<f64>() {
                    return n >= 20.0;
                }
            }
        }
    }
    model.size_bytes > 12_000_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_large_model_by_params() {
        let model = DiscoveredModel {
            name: "test".into(),
            path: "test.gguf".into(),
            mmproj: None,
            format: crate::models::ModelFormat::Gguf,
            size_bytes: 0,
            quant: None,
            param_hint: Some("27B".into()),
            source: crate::models::ModelSource::ExtraDir,
        };
        assert!(is_large_model(&model));
    }

    #[test]
    fn is_not_large_model_by_params() {
        let model = DiscoveredModel {
            name: "test".into(),
            path: "test.gguf".into(),
            mmproj: None,
            format: crate::models::ModelFormat::Gguf,
            size_bytes: 0,
            quant: None,
            param_hint: Some("9B".into()),
            source: crate::models::ModelSource::ExtraDir,
        };
        assert!(!is_large_model(&model));
    }

    #[test]
    fn is_large_model_by_size_fallback() {
        let model = DiscoveredModel {
            name: "test".into(),
            path: "test.gguf".into(),
            mmproj: None,
            format: crate::models::ModelFormat::Gguf,
            size_bytes: 15_000_000_000,
            quant: None,
            param_hint: None,
            source: crate::models::ModelSource::ExtraDir,
        };
        assert!(is_large_model(&model));
    }

    #[test]
    fn lmstudio_backend_returns_error() {
        let config = Config::default();
        let model = DiscoveredModel {
            name: "test".into(),
            path: "test.gguf".into(),
            mmproj: None,
            format: crate::models::ModelFormat::Gguf,
            size_bytes: 0,
            quant: None,
            param_hint: None,
            source: crate::models::ModelSource::ExtraDir,
        };
        let result = launch(&model, &Backend::LmStudio, &config);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.contains("LM Studio"));
    }

    #[test]
    fn ollama_rejects_local_gguf() {
        let config = Config::default();
        let model = DiscoveredModel {
            name: "test".into(),
            path: "test.gguf".into(),
            mmproj: None,
            format: crate::models::ModelFormat::Gguf,
            size_bytes: 0,
            quant: None,
            param_hint: None,
            source: crate::models::ModelSource::ExtraDir,
        };
        let result = launch(&model, &Backend::Ollama, &config);
        assert!(result.is_err());
        assert!(result.err().unwrap().contains("registry"));
    }

    #[test]
    fn vllm_rejects_local_gguf() {
        let config = Config::default();
        let model = DiscoveredModel {
            name: "test".into(),
            path: "test.gguf".into(),
            mmproj: None,
            format: crate::models::ModelFormat::Gguf,
            size_bytes: 0,
            quant: None,
            param_hint: None,
            source: crate::models::ModelSource::ExtraDir,
        };
        let result = launch(&model, &Backend::Vllm, &config);
        assert!(result.is_err());
        assert!(result.err().unwrap().contains("HuggingFace"));
    }
}
