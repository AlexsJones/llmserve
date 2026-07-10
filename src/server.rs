use crate::backends::Backend;
use crate::config::{Config, ResolvedPreset};
use crate::models::DiscoveredModel;
use std::collections::VecDeque;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

const MAX_LOG_LINES: usize = 200;

pub struct ServerHandle {
    pub backend: Backend,
    pub model_name: String,
    pub pid: u32,
    pub port: u16,
    pub host: String,
    pub child: Child,
    pub started_at: Instant,
    /// Ring buffer of log lines (combined stdout + stderr).
    pub log_lines: VecDeque<String>,
    /// Partial line buffer for incomplete reads.
    partial: String,
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

    /// Read any available output from stderr (non-blocking).
    pub fn drain_output(&mut self) {
        let Some(stderr) = self.child.stderr.as_mut() else {
            return;
        };

        let mut buf = [0u8; 4096];
        // Non-blocking read — will return WouldBlock if nothing available
        loop {
            match stderr.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    self.partial.push_str(&chunk);

                    // Split into complete lines
                    while let Some(pos) = self.partial.find('\n') {
                        let line: String = self.partial.drain(..=pos).collect();
                        let line = line.trim_end_matches('\n').trim_end_matches('\r');
                        self.log_lines.push_back(line.to_string());
                        if self.log_lines.len() > MAX_LOG_LINES {
                            self.log_lines.pop_front();
                        }
                    }

                    // Handle \r (carriage return) for progress lines
                    if self.partial.contains('\r') {
                        let last = self.partial.rsplit('\r').next().unwrap_or("").to_string();
                        if !last.is_empty() {
                            // Replace last line if it was a progress update
                            if let Some(back) = self.log_lines.back_mut() {
                                if back.contains('\r') || back.contains('%') || back.contains("...")
                                {
                                    *back = last.clone();
                                } else {
                                    self.log_lines.push_back(last.clone());
                                }
                            } else {
                                self.log_lines.push_back(last.clone());
                            }
                        }
                        self.partial.clear();
                        self.partial.push_str(&last);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        // Also try stdout
        let Some(stdout) = self.child.stdout.as_mut() else {
            return;
        };
        loop {
            match stdout.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    for line in chunk.lines() {
                        self.log_lines.push_back(line.to_string());
                        if self.log_lines.len() > MAX_LOG_LINES {
                            self.log_lines.pop_front();
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }
}

fn set_nonblocking(child: &mut Child) {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        if let Some(ref stderr) = child.stderr {
            let fd = stderr.as_raw_fd();
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
        if let Some(ref stdout) = child.stdout {
            let fd = stdout.as_raw_fd();
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
    }

    // Windows: would need winapi/windows-sys crate for SetNamedPipeHandleState(PIPE_NOWAIT)
    #[cfg(windows)]
    let _ = child;
}

fn make_handle(
    backend: Backend,
    model: &DiscoveredModel,
    port: u16,
    host: String,
    mut child: Child,
) -> ServerHandle {
    set_nonblocking(&mut child);
    let pid = child.id();
    ServerHandle {
        backend,
        model_name: model.name.clone(),
        pid,
        port,
        host,
        child,
        started_at: Instant::now(),
        log_lines: VecDeque::new(),
        partial: String::new(),
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

    Ok(make_handle(
        Backend::LlamaServer,
        model,
        preset.port,
        preset.host.clone(),
        child,
    ))
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

    Ok(make_handle(
        Backend::MlxLm,
        model,
        preset.port,
        preset.host.clone(),
        child,
    ))
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

    Ok(make_handle(
        Backend::KoboldCpp,
        model,
        preset.port,
        preset.host.clone(),
        child,
    ))
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

    Ok(make_handle(
        Backend::LocalAi,
        model,
        preset.port,
        preset.host.clone(),
        child,
    ))
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

    Ok(make_handle(
        Backend::Lemonade,
        model,
        preset.port,
        preset.host.clone(),
        child,
    ))
}

pub fn stop(handle: &mut ServerHandle) {
    // Drain any remaining output before killing
    handle.drain_output();
    let _ = handle.child.kill();
    let _ = handle.child.wait();
}

pub fn check_exited(handle: &mut ServerHandle) -> Option<String> {
    match handle.child.try_wait() {
        Ok(Some(status)) => {
            // Final drain
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
