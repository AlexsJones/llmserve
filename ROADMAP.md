# Roadmap

llmserve is at 0.0.x. These are the feature sets for 0.1, 0.2, and 1.0. Each item names the code it builds on. Nothing here is a promise about a date.

## 0.1 — Stable local serving

The TUI already discovers models, detects nine backends, and `launch_with` in `src/server.rs` starts llama-server, MLX, KoboldCpp, LocalAI, and Lemonade against a file on disk. Presets, the port check (`port_is_free`), the log ring, the `/health` readiness probe, and crash restart (`App::restart_exited`) are in place. 0.1 is that path made safe to leave running.

- **A bad config is an error.** `Config::load` returns the defaults only when the file is missing. A file that cannot be read or parsed is reported before the TUI starts, and `Config::save` leaves that file unchanged.
- **Only offer a backend that can launch the selected entry.** Registry rows use placeholder paths (`ollama:<name>`, `lmstudio:<id>`, `lemonade:<id>`, `fastflowlm:<id>`). The serve dialog cycles only backends for which `Backend::can_serve` is true, and `launch_with` refuses the placeholder before `spawn`. A GGUF file LM Studio stored on disk stays a normal file and can still be launched with llama-server or KoboldCpp.
- **Bind to loopback unless asked.** `preferred_host` is `127.0.0.1`. A partial config that omits the field gets the same value, because `#[serde(default)]` fills missing fields from `Config::default`. `launch_mlx` passes `--host` from `preset.host`, as llama-server, KoboldCpp, and Lemonade already do.
- **Stop the process group.** `server::stop` sends `SIGTERM` to `handle.pid` and then `child.kill()`. llama-server and `python3 -m mlx_lm.server` can outlive that pid. On Unix, put the child in its own group at `spawn` (`setpgid`) and signal the group. On Windows, put it in a Job Object and kill the job. `s`, `S`, and quit should all use that path.
- **One set of discovery paths.** `discover_models` on Windows checks `%USERPROFILE%\.lmstudio\models`, then `%LOCALAPPDATA%\LM Studio\models`, and the same split for llama.cpp (`%LOCALAPPDATA%\llm-models`). `build_tree` in `src/app.rs` still always shows `~/.lmstudio/models` and `~/.cache/llm-models`. The tree should call the same helpers discovery uses, so a model that was found is visible under the directory it came from.
- **A serve test in CI.** Compile a tiny stand-in for `llama-server`, point the binary-override hook at it, `launch_with` a fake GGUF path, wait until `probe_ready` sees `GET /health`, assert `--model`, `--host`, and `--port`, then `stop`. No real model and no real engine. CI builds with `-D warnings` and `--locked`.

## 0.2 — Headless use, Hugging Face GGUF, Ollama and vLLM

`main` accepts `--help` and `--version` and exits 2 on anything else. Ollama, LM Studio, vLLM, and FastFlowLM are detected and `fetch_api_models` merges their lists into the table, but `launch_with` returns an error for all four. `scan_mlx_models` only keeps Hugging Face repos whose folder name contains `mlx`. The llmfit cache at `~/.cache/llmfit/models` is already scanned; there is no way for llmfit to hand over a chosen file except the user picking it again in the TUI.

- **`llmserve list`.** Print discovered models as text: name, format, source, size, path. Reuse `discover_models` and the same Ollama, LM Studio, Lemonade, and FastFlowLM fetchers the TUI uses. No alternate screen.
- **`llmserve serve --model <path> --backend <key> [--host] [--port]`.** Resolve the preset with `Config::preset_for`, call `launch_with`, write the log lines to stdout, and `stop` on Ctrl-C. This is the whole headless path. It is also the llmfit handoff: llmfit already downloads GGUF into `~/.cache/llmfit/models`, which discovery reads, so llmfit only needs to exec this command with the path it picked. No second scanner.
- **Hugging Face GGUF.** `hf_hub_dir` already resolves `$HF_HOME/hub` or `~/.cache/huggingface/hub`. Walk that tree with `scan_gguf_dir` for `*.gguf`, in addition to the existing MLX snapshot scan. A downloaded GGUF is a normal file, so llama-server, KoboldCpp, LocalAI, and Lemonade serve it with the launchers that exist now.
- **Ollama serving.** For an `ollama:<name>` entry, use the daemon `detect_ollama` already found (`OLLAMA_HOST`, default `http://localhost:11434`). Do not spawn a second Ollama. Load the name through the daemon's HTTP API and track that URL as the running server (readiness can poll the same `/api/tags` probe detection uses). `stop` then unloads or simply detaches, and says which. llama-server is still not a valid choice for that row.
- **vLLM serving.** When `find_binary("vllm")` succeeds, run `vllm serve <hf-id> --host <host> --port <port>` from the vLLM preset (default port 8000). The id comes from Hugging Face discovery or from `--model` when the argument is an id rather than a GGUF path. `local_serve_reason` already says vLLM does not load local GGUF; 0.2 does not change that.
- **LM Studio and FastFlowLM stay list-only.** `local_serve_reason` says LM Studio manages its own server and FastFlowLM uses its own NPU registry. 0.2 keeps showing their API models and does not grow a launcher until `lms` or `flm` has a load command that takes the id after the `lmstudio:` / `fastflowlm:` prefix.

## 1.0 — The TUI and the CLI are the same server

- The serve dialog and `llmserve serve` call one function: preset resolution, `port_is_free`, `launch_with`, the log ring, and `probe_ready`. A flag in the dialog and a flag on the CLI change the same field.
- Stop is the process-group / Job Object path from 0.1, used by `s`, `S`, quit, and Ctrl-C in `llmserve serve`.
- The binary-override hook used for llama-server in CI also drives a fake `ollama` HTTP daemon and a fake `vllm`, so those two launch paths break the build when they regress.
- The llmfit handoff is the supported one: `llmserve serve --model <path> --backend llama-server`. Document it next to the companion-project section. llmfit does not need a private socket or a shared config schema.
- Config stays fail-closed. Adding or renaming a preset field is a changelog entry, because `#[serde(default)]` will otherwise silently fill the new field and a save will rewrite the user's file with that default.
