use crate::backends::{
    Backend, DetectedBackend, detect_backends, fetch_fastflowlm_models, fetch_lemonade_models,
    fetch_lmstudio_models, fetch_ollama_models,
};
use crate::config::{Config, ResolvedPreset};
use crate::models::{
    DiscoveredModel, ModelFormat, ModelSource, add_fastflowlm_models, add_lemonade_models,
    add_lmstudio_models, add_ollama_models, discover_models,
};
use crate::server::{self, ServerHandle};
use crate::theme::Theme;
use std::collections::VecDeque;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
    BackendPopup,
    ConfirmServe,
    StopPopup,
    AddDir,
    ServerExit,
}

/// Editable fields in the confirm-serve modal, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmField {
    Backend,
    Port,
    CtxSize,
    FlashAttn,
    GpuLayers,
    Threads,
    BatchSize,
    ExtraArgs,
}

pub const CONFIRM_FIELDS: [ConfirmField; 8] = [
    ConfirmField::Backend,
    ConfirmField::Port,
    ConfirmField::CtxSize,
    ConfirmField::FlashAttn,
    ConfirmField::GpuLayers,
    ConfirmField::Threads,
    ConfirmField::BatchSize,
    ConfirmField::ExtraArgs,
];

/// Details about a server that exited on its own, shown in a popup modal.
#[derive(Debug, Clone)]
pub struct ExitInfo {
    pub model_name: String,
    pub backend_label: String,
    pub port: u16,
    pub message: String,
    pub log_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Table,
    Serve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatFilter {
    All,
    Gguf,
    Mlx,
}

impl FormatFilter {
    pub fn next(&self) -> Self {
        match self {
            FormatFilter::All => FormatFilter::Gguf,
            FormatFilter::Gguf => FormatFilter::Mlx,
            FormatFilter::Mlx => FormatFilter::All,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            FormatFilter::All => "All",
            FormatFilter::Gguf => "GGUF",
            FormatFilter::Mlx => "MLX",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Name,
    Size,
    Source,
}

impl SortOrder {
    pub fn next(&self) -> Self {
        match self {
            SortOrder::Name => SortOrder::Size,
            SortOrder::Size => SortOrder::Source,
            SortOrder::Source => SortOrder::Name,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            SortOrder::Name => "Name",
            SortOrder::Size => "Size",
            SortOrder::Source => "Source",
        }
    }
}

// -- Source tree --

/// A node in the source tree. Each represents a source category or a custom directory.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub label: String,
    pub path: Option<PathBuf>,
    pub source: Option<ModelSource>,
    pub model_count: usize,
    pub expanded: bool,
    pub removable: bool,
    /// Models belonging to this node (indices into App.models).
    pub model_indices: Vec<usize>,
}

pub struct App {
    pub input_mode: InputMode,
    pub focus: Focus,
    pub should_quit: bool,

    pub models: Vec<DiscoveredModel>,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub visible_rows: usize,

    pub search_query: String,
    pub format_filter: FormatFilter,
    pub sort_order: SortOrder,

    pub backends: Vec<DetectedBackend>,
    pub selected_backend: usize,
    pub backend_popup_cursor: usize,

    pub servers: Vec<ServerHandle>,
    pub stop_popup_cursor: usize,
    /// Log lines from servers that have exited (so we can see crash output).
    pub dead_logs: VecDeque<String>,
    /// Queue of unexpected server exits awaiting acknowledgement via popup.
    pub exit_popups: VecDeque<ExitInfo>,
    /// Scroll offset (lines up from the bottom) within the exit popup log view.
    pub exit_popup_scroll: usize,

    pub confirm_backend_idx: usize,
    pub confirm_port_input: String,
    /// Working copy of the launch settings, seeded from the backend's preset.
    pub confirm_preset: ResolvedPreset,
    /// Index into CONFIRM_FIELDS of the selected field.
    pub confirm_field: usize,
    /// Whether the selected field is being text-edited.
    pub confirm_editing: bool,
    pub confirm_edit_buf: String,

    // Source tree
    pub tree_nodes: Vec<TreeNode>,
    pub tree_cursor: usize,
    pub tree_source_filter: Option<ModelSource>,

    /// Width of the left source tree panel.
    pub tree_width: u16,
    /// Width of the right serve/logs panel.
    pub serve_width: u16,
    /// Whether log output wraps long lines.
    pub log_wrap: bool,
    /// Whether the source tree panel is visible.
    pub show_tree: bool,
    /// Whether the serve/logs panel is visible (user can toggle even when empty).
    pub show_serve: bool,

    // Add directory input
    pub add_dir_input: String,
    /// Tab-completion candidates for add-dir.
    pub add_dir_completions: Vec<String>,
    /// Currently selected completion index.
    pub add_dir_completion_idx: usize,

    pub status_message: Option<String>,

    pub config: Config,
    pub theme: Theme,
}

impl App {
    pub fn new() -> Self {
        let config = Config::load();
        let theme = Theme::load();
        let backends = detect_backends();

        let mut models = discover_models(&config.extra_model_dirs);

        if let Some(ollama) = backends.iter().find(|b| b.backend == Backend::Ollama) {
            if ollama.available {
                if let Some(ref url) = ollama.api_url {
                    let ollama_models = fetch_ollama_models(url);
                    add_ollama_models(&mut models, ollama_models);
                }
            }
        }

        if let Some(lmstudio) = backends.iter().find(|b| b.backend == Backend::LmStudio) {
            if lmstudio.available {
                if let Some(ref url) = lmstudio.api_url {
                    let lmstudio_models = fetch_lmstudio_models(url);
                    add_lmstudio_models(&mut models, lmstudio_models);
                }
            }
        }

        if let Some(lemonade) = backends.iter().find(|b| b.backend == Backend::Lemonade) {
            if lemonade.available {
                if let Some(ref url) = lemonade.api_url {
                    let lemonade_models = fetch_lemonade_models(url);
                    add_lemonade_models(&mut models, lemonade_models);
                }
            }
        }

        if let Some(flm) = backends.iter().find(|b| b.backend == Backend::FastFlowLm) {
            if flm.available {
                if let Some(ref url) = flm.api_url {
                    let flm_models = fetch_fastflowlm_models(url);
                    add_fastflowlm_models(&mut models, flm_models);
                }
            }
        }

        let selected_backend = if let Some(ref pref) = config.default_backend {
            backends
                .iter()
                .position(|b| {
                    b.backend.label().to_lowercase() == pref.to_lowercase() && b.available
                })
                .unwrap_or_else(|| first_available(&backends))
        } else {
            first_available(&backends)
        };

        let filtered: Vec<usize> = (0..models.len()).collect();
        let tree_nodes = build_tree(&models, &config);

        App {
            input_mode: InputMode::Normal,
            focus: Focus::Table,
            should_quit: false,
            models,
            filtered,
            selected: 0,
            scroll_offset: 0,
            visible_rows: 20,
            search_query: String::new(),
            format_filter: FormatFilter::All,
            sort_order: SortOrder::Name,
            backends,
            selected_backend,
            backend_popup_cursor: selected_backend,
            servers: Vec::new(),
            stop_popup_cursor: 0,
            dead_logs: VecDeque::new(),
            exit_popups: VecDeque::new(),
            exit_popup_scroll: 0,
            confirm_backend_idx: selected_backend,
            confirm_port_input: String::new(),
            confirm_preset: config.preset_for("llama-server"),
            confirm_field: 0,
            confirm_editing: false,
            confirm_edit_buf: String::new(),
            tree_nodes,
            tree_cursor: 0,
            tree_source_filter: None,
            tree_width: 30,
            serve_width: 38,
            log_wrap: false,
            show_tree: true,
            show_serve: false,
            add_dir_input: String::new(),
            add_dir_completions: Vec::new(),
            add_dir_completion_idx: 0,
            status_message: None,
            config,
            theme,
        }
    }

    pub fn active_backend(&self) -> Option<&DetectedBackend> {
        self.backends.get(self.selected_backend)
    }

    pub fn selected_model(&self) -> Option<&DiscoveredModel> {
        self.filtered.get(self.selected).map(|&i| &self.models[i])
    }

    pub fn is_model_served(&self, model_name: &str) -> bool {
        self.servers.iter().any(|s| s.model_name == model_name)
    }

    pub fn next_available_port(&self) -> u16 {
        let base = self.config.preferred_port;
        let used: std::collections::HashSet<u16> = self.servers.iter().map(|s| s.port).collect();
        (base..).find(|p| !used.contains(p)).unwrap_or(base)
    }

    // -- Focus --

    pub fn toggle_focus(&mut self) {
        // Build the list of visible panels and cycle through them
        let mut panels = Vec::new();
        if self.show_tree {
            panels.push(Focus::Tree);
        }
        panels.push(Focus::Table); // table is always visible
        if self.show_serve {
            panels.push(Focus::Serve);
        }

        if let Some(pos) = panels.iter().position(|&f| f == self.focus) {
            self.focus = panels[(pos + 1) % panels.len()];
        } else {
            self.focus = Focus::Table;
        }
    }

    pub fn toggle_tree(&mut self) {
        self.show_tree = !self.show_tree;
        if !self.show_tree && self.focus == Focus::Tree {
            self.focus = Focus::Table;
        }
    }

    pub fn toggle_serve_panel(&mut self) {
        self.show_serve = !self.show_serve;
        if !self.show_serve && self.focus == Focus::Serve {
            self.focus = Focus::Table;
        }
    }

    // -- Navigation --

    pub fn move_down(&mut self) {
        match self.focus {
            Focus::Table => {
                if !self.filtered.is_empty() && self.selected < self.filtered.len() - 1 {
                    self.selected += 1;
                    self.ensure_visible();
                }
            }
            Focus::Tree => {
                if self.tree_cursor < self.tree_nodes.len().saturating_sub(1) {
                    self.tree_cursor += 1;
                }
            }
            Focus::Serve => {}
        }
    }

    pub fn move_up(&mut self) {
        match self.focus {
            Focus::Table => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.ensure_visible();
                }
            }
            Focus::Tree => {
                if self.tree_cursor > 0 {
                    self.tree_cursor -= 1;
                }
            }
            Focus::Serve => {}
        }
    }

    pub fn home(&mut self) {
        match self.focus {
            Focus::Table => {
                self.selected = 0;
                self.scroll_offset = 0;
            }
            Focus::Tree => {
                self.tree_cursor = 0;
            }
            Focus::Serve => {}
        }
    }

    pub fn end(&mut self) {
        match self.focus {
            Focus::Table => {
                if !self.filtered.is_empty() {
                    self.selected = self.filtered.len() - 1;
                    self.ensure_visible();
                }
            }
            Focus::Tree => {
                if !self.tree_nodes.is_empty() {
                    self.tree_cursor = self.tree_nodes.len() - 1;
                }
            }
            Focus::Serve => {}
        }
    }

    pub fn half_page_down(&mut self) {
        if self.focus == Focus::Table {
            let jump = self.visible_rows / 2;
            self.selected = (self.selected + jump).min(self.filtered.len().saturating_sub(1));
            self.ensure_visible();
        }
    }

    pub fn half_page_up(&mut self) {
        if self.focus == Focus::Table {
            let jump = self.visible_rows / 2;
            self.selected = self.selected.saturating_sub(jump);
            self.ensure_visible();
        }
    }

    fn ensure_visible(&mut self) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + self.visible_rows {
            self.scroll_offset = self.selected - self.visible_rows + 1;
        }
    }

    // -- Pane resize (Shift+Left/Right grows/shrinks the focused pane) --

    pub fn grow_focused_pane(&mut self) {
        match self.focus {
            Focus::Tree => {
                if self.tree_width < 60 {
                    self.tree_width += 2;
                }
            }
            Focus::Serve => {
                if self.serve_width < 80 {
                    self.serve_width += 2;
                }
            }
            Focus::Table => {} // table uses remaining space
        }
    }

    pub fn shrink_focused_pane(&mut self) {
        match self.focus {
            Focus::Tree => {
                if self.tree_width > 16 {
                    self.tree_width -= 2;
                }
            }
            Focus::Serve => {
                if self.serve_width > 24 {
                    self.serve_width -= 2;
                }
            }
            Focus::Table => {} // table uses remaining space
        }
    }

    pub fn toggle_log_wrap(&mut self) {
        self.log_wrap = !self.log_wrap;
    }

    // -- Tree actions --

    pub fn tree_toggle_expand(&mut self) {
        if let Some(node) = self.tree_nodes.get_mut(self.tree_cursor) {
            node.expanded = !node.expanded;
        }
    }

    pub fn tree_select_source(&mut self) {
        if let Some(node) = self.tree_nodes.get(self.tree_cursor) {
            if node.source.is_some() {
                if self.tree_source_filter == node.source {
                    // Deselect — show all
                    self.tree_source_filter = None;
                } else {
                    self.tree_source_filter = node.source.clone();
                }
                self.apply_filters();
            }
        }
    }

    pub fn tree_remove_dir(&mut self) {
        let Some(node) = self.tree_nodes.get(self.tree_cursor) else {
            return;
        };
        if !node.removable {
            self.status_message = Some("Cannot remove built-in source".into());
            return;
        }
        let Some(path) = node.path.clone() else {
            return;
        };
        self.config.extra_model_dirs.retain(|d| *d != path);
        self.config.save();
        self.rebuild_models();
        self.status_message = Some(format!("Removed {}", path.display()));
    }

    pub fn open_add_dir(&mut self) {
        self.add_dir_input.clear();
        self.add_dir_completions.clear();
        self.add_dir_completion_idx = 0;
        self.input_mode = InputMode::AddDir;
    }

    pub fn add_dir_push(&mut self, c: char) {
        self.add_dir_input.push(c);
        self.refresh_completions();
    }

    pub fn add_dir_pop(&mut self) {
        self.add_dir_input.pop();
        self.refresh_completions();
    }

    pub fn add_dir_accept_completion(&mut self) {
        if let Some(completion) = self.add_dir_completions.get(self.add_dir_completion_idx) {
            self.add_dir_input = completion.clone();
            // Append / if it's a directory so user can keep drilling
            if !self.add_dir_input.ends_with('/') {
                self.add_dir_input.push('/');
            }
            self.refresh_completions();
        }
    }

    pub fn add_dir_next_completion(&mut self) {
        if !self.add_dir_completions.is_empty() {
            self.add_dir_completion_idx =
                (self.add_dir_completion_idx + 1) % self.add_dir_completions.len();
        }
    }

    pub fn add_dir_prev_completion(&mut self) {
        if !self.add_dir_completions.is_empty() {
            self.add_dir_completion_idx = if self.add_dir_completion_idx == 0 {
                self.add_dir_completions.len() - 1
            } else {
                self.add_dir_completion_idx - 1
            };
        }
    }

    fn refresh_completions(&mut self) {
        self.add_dir_completions = compute_completions(&self.add_dir_input);
        self.add_dir_completion_idx = 0;
    }

    pub fn confirm_add_dir(&mut self) {
        let raw = self.add_dir_input.trim().to_string();
        if raw.is_empty() {
            self.input_mode = InputMode::Normal;
            return;
        }

        // Expand ~ to home dir
        let expanded = if raw.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                home.join(raw.strip_prefix("~/").unwrap_or(&raw[1..]))
            } else {
                PathBuf::from(&raw)
            }
        } else {
            PathBuf::from(&raw)
        };

        if !expanded.is_dir() {
            self.status_message = Some(format!("Not a directory: {}", expanded.display()));
            self.input_mode = InputMode::Normal;
            return;
        }

        if self.config.extra_model_dirs.contains(&expanded) {
            self.status_message = Some("Directory already added".into());
            self.input_mode = InputMode::Normal;
            return;
        }

        self.config.extra_model_dirs.push(expanded.clone());
        self.config.save();
        self.rebuild_models();
        self.status_message = Some(format!("Added {}", expanded.display()));
        self.input_mode = InputMode::Normal;
    }

    pub fn cancel_add_dir(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    // -- Filtering --

    pub fn apply_filters(&mut self) {
        self.filtered = (0..self.models.len())
            .filter(|&i| {
                let m = &self.models[i];
                // Source filter from tree
                if let Some(ref src) = self.tree_source_filter {
                    if m.source != *src {
                        return false;
                    }
                }
                match self.format_filter {
                    FormatFilter::All => {}
                    FormatFilter::Gguf => {
                        if m.format != ModelFormat::Gguf {
                            return false;
                        }
                    }
                    FormatFilter::Mlx => {
                        if m.format != ModelFormat::Mlx {
                            return false;
                        }
                    }
                }
                if !self.search_query.is_empty() {
                    let q = self.search_query.to_lowercase();
                    let name_lower = m.name.to_lowercase();
                    if !name_lower.contains(&q) {
                        return false;
                    }
                }
                true
            })
            .collect();

        match self.sort_order {
            SortOrder::Name => {
                self.filtered.sort_by(|&a, &b| {
                    self.models[a]
                        .name
                        .to_lowercase()
                        .cmp(&self.models[b].name.to_lowercase())
                });
            }
            SortOrder::Size => {
                self.filtered
                    .sort_by(|&a, &b| self.models[b].size_bytes.cmp(&self.models[a].size_bytes));
            }
            SortOrder::Source => {
                self.filtered.sort_by(|&a, &b| {
                    self.models[a]
                        .source
                        .to_string()
                        .cmp(&self.models[b].source.to_string())
                });
            }
        }

        if self.filtered.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len() - 1;
        }
        self.scroll_offset = 0;
    }

    pub fn cycle_format_filter(&mut self) {
        self.format_filter = self.format_filter.next();
        self.apply_filters();
    }

    pub fn cycle_sort(&mut self) {
        self.sort_order = self.sort_order.next();
        self.apply_filters();
    }

    // -- Search --

    pub fn enter_search(&mut self) {
        self.input_mode = InputMode::Search;
    }

    pub fn search_push(&mut self, c: char) {
        self.search_query.push(c);
        self.apply_filters();
    }

    pub fn search_pop(&mut self) {
        self.search_query.pop();
        self.apply_filters();
    }

    pub fn exit_search(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.apply_filters();
        self.input_mode = InputMode::Normal;
    }

    // -- Backend popup --

    pub fn open_backend_popup(&mut self) {
        self.backend_popup_cursor = self.selected_backend;
        self.input_mode = InputMode::BackendPopup;
    }

    pub fn backend_popup_down(&mut self) {
        if self.backend_popup_cursor < self.backends.len() - 1 {
            self.backend_popup_cursor += 1;
        }
    }

    pub fn backend_popup_up(&mut self) {
        if self.backend_popup_cursor > 0 {
            self.backend_popup_cursor -= 1;
        }
    }

    pub fn select_backend(&mut self) {
        self.selected_backend = self.backend_popup_cursor;
        self.input_mode = InputMode::Normal;
    }

    // -- Serve --

    pub fn open_confirm_serve(&mut self) {
        let Some(model) = self.selected_model() else {
            return;
        };
        let format = model.format.clone();

        // Pre-select the first compatible + available backend
        let best = self
            .backends
            .iter()
            .position(|b| b.available && b.backend.can_serve_local(&format))
            .unwrap_or(self.selected_backend);

        self.confirm_backend_idx = best;
        self.confirm_port_input = self.next_available_port().to_string();
        self.confirm_field = 0;
        self.confirm_editing = false;
        self.confirm_edit_buf.clear();
        self.confirm_reseed_preset();
        self.input_mode = InputMode::ConfirmServe;
    }

    /// Re-seed the working preset from the currently selected backend's config.
    fn confirm_reseed_preset(&mut self) {
        if let Some(b) = self.backends.get(self.confirm_backend_idx) {
            let key = crate::backends::backend_key(&b.backend);
            self.confirm_preset = self.config.preset_for(key);
        }
    }

    pub fn confirm_backend(&self) -> Option<&DetectedBackend> {
        self.backends.get(self.confirm_backend_idx)
    }

    pub fn confirm_port(&self) -> u16 {
        self.confirm_port_input
            .parse()
            .unwrap_or(self.next_available_port())
    }

    /// Whether the selected backend is compatible with the selected model's format.
    pub fn confirm_compatible(&self) -> bool {
        let Some(model) = self.selected_model() else {
            return false;
        };
        let Some(backend) = self.confirm_backend() else {
            return false;
        };
        backend.backend.can_serve_local(&model.format)
    }

    /// Reason the selected backend can't serve local files, if any.
    pub fn confirm_incompatible_reason(&self) -> Option<&'static str> {
        self.confirm_backend()
            .and_then(|b| b.backend.local_serve_reason())
    }

    pub fn confirm_already_serving(&self) -> bool {
        let Some(model) = self.selected_model() else {
            return false;
        };
        let Some(backend) = self.confirm_backend() else {
            return false;
        };
        self.servers
            .iter()
            .any(|s| s.model_name == model.name && s.backend == backend.backend)
    }

    pub fn confirm_cycle_backend_right(&mut self) {
        if !self.backends.is_empty() {
            self.confirm_backend_idx = (self.confirm_backend_idx + 1) % self.backends.len();
            self.confirm_reseed_preset();
        }
    }

    pub fn confirm_cycle_backend_left(&mut self) {
        if !self.backends.is_empty() {
            self.confirm_backend_idx = if self.confirm_backend_idx == 0 {
                self.backends.len() - 1
            } else {
                self.confirm_backend_idx - 1
            };
            self.confirm_reseed_preset();
        }
    }

    pub fn confirm_selected_field(&self) -> ConfirmField {
        CONFIRM_FIELDS[self.confirm_field.min(CONFIRM_FIELDS.len() - 1)]
    }

    pub fn confirm_field_down(&mut self) {
        self.confirm_field = (self.confirm_field + 1) % CONFIRM_FIELDS.len();
    }

    pub fn confirm_field_up(&mut self) {
        self.confirm_field = if self.confirm_field == 0 {
            CONFIRM_FIELDS.len() - 1
        } else {
            self.confirm_field - 1
        };
    }

    /// h/l or Space on the selected field: cycle backend or flip a toggle.
    pub fn confirm_field_left(&mut self) {
        match self.confirm_selected_field() {
            ConfirmField::Backend => self.confirm_cycle_backend_left(),
            _ => self.confirm_toggle(),
        }
    }

    pub fn confirm_field_right(&mut self) {
        match self.confirm_selected_field() {
            ConfirmField::Backend => self.confirm_cycle_backend_right(),
            _ => self.confirm_toggle(),
        }
    }

    pub fn confirm_toggle(&mut self) {
        match self.confirm_selected_field() {
            ConfirmField::CtxSize => {
                self.confirm_preset.use_ctx_size = !self.confirm_preset.use_ctx_size;
            }
            ConfirmField::FlashAttn => {
                self.confirm_preset.flash_attn = !self.confirm_preset.flash_attn;
            }
            _ => {}
        }
    }

    pub fn confirm_begin_edit(&mut self) {
        let buf = match self.confirm_selected_field() {
            // Not text-editable
            ConfirmField::Backend | ConfirmField::FlashAttn => return,
            ConfirmField::Port => self.confirm_port_input.clone(),
            ConfirmField::CtxSize => self.confirm_preset.ctx_size.to_string(),
            ConfirmField::GpuLayers => self
                .confirm_preset
                .gpu_layers
                .map(|v| v.to_string())
                .unwrap_or_default(),
            ConfirmField::Threads => self
                .confirm_preset
                .threads
                .map(|v| v.to_string())
                .unwrap_or_default(),
            ConfirmField::BatchSize => self
                .confirm_preset
                .batch_size
                .map(|v| v.to_string())
                .unwrap_or_default(),
            ConfirmField::ExtraArgs => self.confirm_preset.extra_args.join(" "),
        };
        self.confirm_edit_buf = buf;
        self.confirm_editing = true;
    }

    pub fn confirm_edit_push(&mut self, c: char) {
        let ok = match self.confirm_selected_field() {
            ConfirmField::Port => c.is_ascii_digit() && self.confirm_edit_buf.len() < 5,
            ConfirmField::CtxSize | ConfirmField::Threads | ConfirmField::BatchSize => {
                c.is_ascii_digit() && self.confirm_edit_buf.len() < 9
            }
            ConfirmField::GpuLayers => {
                (c.is_ascii_digit() || (c == '-' && self.confirm_edit_buf.is_empty()))
                    && self.confirm_edit_buf.len() < 5
            }
            ConfirmField::ExtraArgs => !c.is_control(),
            ConfirmField::Backend | ConfirmField::FlashAttn => false,
        };
        if ok {
            self.confirm_edit_buf.push(c);
        }
    }

    pub fn confirm_edit_pop(&mut self) {
        self.confirm_edit_buf.pop();
    }

    /// Commit the edit buffer into the working preset. Empty optional fields
    /// become "auto" (None); unparseable input keeps the previous value.
    pub fn confirm_commit_edit(&mut self) {
        let buf = self.confirm_edit_buf.trim().to_string();
        match self.confirm_selected_field() {
            ConfirmField::Port => {
                if buf.parse::<u16>().is_ok() {
                    self.confirm_port_input = buf;
                }
            }
            ConfirmField::CtxSize => {
                if let Ok(v) = buf.parse() {
                    self.confirm_preset.ctx_size = v;
                }
            }
            ConfirmField::GpuLayers => self.confirm_preset.gpu_layers = buf.parse().ok(),
            ConfirmField::Threads => self.confirm_preset.threads = buf.parse().ok(),
            ConfirmField::BatchSize => self.confirm_preset.batch_size = buf.parse().ok(),
            ConfirmField::ExtraArgs => {
                self.confirm_preset.extra_args =
                    buf.split_whitespace().map(str::to_string).collect();
            }
            ConfirmField::Backend | ConfirmField::FlashAttn => {}
        }
        self.confirm_editing = false;
        self.confirm_edit_buf.clear();
    }

    pub fn confirm_cancel_edit(&mut self) {
        self.confirm_editing = false;
        self.confirm_edit_buf.clear();
    }

    /// Persist the working preset as this backend's defaults in config.toml.
    pub fn confirm_save_preset(&mut self) {
        let Some(backend) = self.backends.get(self.confirm_backend_idx) else {
            return;
        };
        let key = crate::backends::backend_key(&backend.backend).to_string();
        let p = &self.confirm_preset;
        let entry = self.config.presets.entry(key.clone()).or_default();
        entry.ctx_size = Some(p.ctx_size);
        entry.use_ctx_size = Some(p.use_ctx_size);
        entry.flash_attn = Some(p.flash_attn);
        entry.batch_size = p.batch_size;
        entry.gpu_layers = p.gpu_layers;
        entry.threads = p.threads;
        entry.extra_args = p.extra_args.clone();
        if let Ok(port) = self.confirm_port_input.parse::<u16>() {
            entry.port = Some(port);
        }
        self.config.save();
        self.status_message = Some(format!("Saved defaults for {key}"));
    }

    pub fn do_serve(&mut self) {
        self.input_mode = InputMode::Normal;

        let Some(model) = self.selected_model().cloned() else {
            return;
        };
        let Some(backend) = self.backends.get(self.confirm_backend_idx) else {
            self.status_message = Some("No backend selected".into());
            return;
        };

        if !backend.available {
            self.status_message = Some(format!("{} is not available", backend.backend.label()));
            return;
        }

        if !backend.backend.can_serve_local(&model.format) {
            let reason = backend
                .backend
                .local_serve_reason()
                .unwrap_or("incompatible format");
            self.status_message = Some(format!("{}: {}", backend.backend.label(), reason));
            return;
        }

        if self.confirm_already_serving() {
            self.status_message = Some(format!(
                "{} is already being served via {}",
                model.name,
                backend.backend.label()
            ));
            return;
        }

        let port = self.confirm_port();
        if self.servers.iter().any(|s| s.port == port) {
            self.status_message = Some(format!("Port {port} is already in use"));
            return;
        }

        let mut preset = self.confirm_preset.clone();
        preset.port = port;

        match server::launch_with(&model, &backend.backend, &preset) {
            Ok(handle) => {
                self.status_message = Some(format!(
                    "Started {} via {} on port {}",
                    handle.model_name,
                    handle.backend.label(),
                    handle.port
                ));
                self.servers.push(handle);
                self.show_serve = true; // auto-show serve panel
            }
            Err(e) => {
                self.status_message = Some(e);
                self.show_serve = true; // show panel so error is visible
            }
        }
    }

    // -- Stop --

    pub fn open_stop_popup(&mut self) {
        if self.servers.is_empty() {
            self.status_message = Some("No servers running".into());
            return;
        }
        if self.servers.len() == 1 {
            self.stop_server_at(0);
            return;
        }
        self.stop_popup_cursor = 0;
        self.input_mode = InputMode::StopPopup;
    }

    pub fn stop_popup_down(&mut self) {
        if self.stop_popup_cursor < self.servers.len() {
            self.stop_popup_cursor += 1;
        }
    }

    pub fn stop_popup_up(&mut self) {
        if self.stop_popup_cursor > 0 {
            self.stop_popup_cursor -= 1;
        }
    }

    pub fn confirm_stop(&mut self) {
        if self.stop_popup_cursor == self.servers.len() {
            self.stop_all_servers();
        } else {
            self.stop_server_at(self.stop_popup_cursor);
        }
        self.input_mode = InputMode::Normal;
    }

    fn stop_server_at(&mut self, idx: usize) {
        if idx < self.servers.len() {
            let mut handle = self.servers.remove(idx);
            let name = handle.model_name.clone();
            let port = handle.port;
            server::stop(&mut handle);
            self.status_message = Some(format!("Stopped {name} on port {port}"));
        }
    }

    pub fn stop_all_servers(&mut self) {
        let count = self.servers.len();
        for handle in &mut self.servers {
            server::stop(handle);
        }
        self.servers.clear();
        self.status_message = Some(format!("Stopped {count} server(s)"));
    }

    pub fn cancel_popup(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    // -- Tick --

    pub fn tick(&mut self) {
        // Drain output from all running servers
        for handle in &mut self.servers {
            handle.drain_output();
        }

        // Check for exits
        let mut exited = Vec::new();
        for (i, handle) in self.servers.iter_mut().enumerate() {
            if let Some(msg) = server::check_exited(handle) {
                exited.push((i, msg));
            }
        }
        for (i, msg) in exited.into_iter().rev() {
            let handle = self.servers.remove(i);
            // Queue a popup so the user sees why the server exited
            self.exit_popups.push_back(ExitInfo {
                model_name: handle.model_name.clone(),
                backend_label: handle.backend.label().to_string(),
                port: handle.port,
                message: msg.clone(),
                log_lines: handle.log_lines.iter().cloned().collect(),
            });
            // Preserve logs from the dead server
            self.dead_logs.push_back(format!(
                "--- {} (:{}) exited: {msg} ---",
                handle.model_name, handle.port
            ));
            for line in &handle.log_lines {
                self.dead_logs.push_back(line.clone());
            }
            // Cap dead logs
            while self.dead_logs.len() > 200 {
                self.dead_logs.pop_front();
            }
            self.status_message = Some(format!(
                "{} (port {}): {msg}",
                handle.model_name, handle.port
            ));
            self.show_serve = true; // make crash logs visible after dismissal
        }

        // Surface the popup once no other modal is active
        if !self.exit_popups.is_empty() && self.input_mode == InputMode::Normal {
            self.exit_popup_scroll = 0;
            self.input_mode = InputMode::ServerExit;
        }
    }

    // -- Server exit popup --

    pub fn current_exit_popup(&self) -> Option<&ExitInfo> {
        self.exit_popups.front()
    }

    pub fn dismiss_exit_popup(&mut self) {
        self.exit_popups.pop_front();
        self.exit_popup_scroll = 0;
        if self.exit_popups.is_empty() {
            self.input_mode = InputMode::Normal;
        }
    }

    pub fn exit_popup_scroll_up(&mut self) {
        let max = self
            .exit_popups
            .front()
            .map(|e| e.log_lines.len())
            .unwrap_or(0);
        if self.exit_popup_scroll < max {
            self.exit_popup_scroll += 1;
        }
    }

    pub fn exit_popup_scroll_down(&mut self) {
        self.exit_popup_scroll = self.exit_popup_scroll.saturating_sub(1);
    }

    /// Get all log lines to display — combines live server logs and dead logs.
    pub fn all_log_lines(&self) -> Vec<(&str, &str)> {
        let mut lines = Vec::new();

        // Dead logs first (historical)
        for line in &self.dead_logs {
            lines.push(("", line.as_str()));
        }

        // Then live server logs
        for s in &self.servers {
            if !s.log_lines.is_empty() {
                lines.push((s.model_name.as_str(), "─── live ───"));
                for line in &s.log_lines {
                    lines.push((s.model_name.as_str(), line.as_str()));
                }
            }
        }

        lines
    }

    pub fn clear_dead_logs(&mut self) {
        self.dead_logs.clear();
    }

    /// Whether there are any logs to show (live or dead).
    pub fn has_logs(&self) -> bool {
        !self.dead_logs.is_empty() || self.servers.iter().any(|s| !s.log_lines.is_empty())
    }

    // -- Refresh / rebuild --

    pub fn refresh(&mut self) {
        self.backends = detect_backends();
        self.rebuild_models();
        self.status_message = Some(format!("Found {} models", self.models.len()));
    }

    fn rebuild_models(&mut self) {
        self.models = discover_models(&self.config.extra_model_dirs);

        if let Some(ollama) = self.backends.iter().find(|b| b.backend == Backend::Ollama) {
            if ollama.available {
                if let Some(ref url) = ollama.api_url {
                    let ollama_models = fetch_ollama_models(url);
                    add_ollama_models(&mut self.models, ollama_models);
                }
            }
        }

        if let Some(lmstudio) = self
            .backends
            .iter()
            .find(|b| b.backend == Backend::LmStudio)
        {
            if lmstudio.available {
                if let Some(ref url) = lmstudio.api_url {
                    let lmstudio_models = fetch_lmstudio_models(url);
                    add_lmstudio_models(&mut self.models, lmstudio_models);
                }
            }
        }

        if let Some(lemonade) = self
            .backends
            .iter()
            .find(|b| b.backend == Backend::Lemonade)
        {
            if lemonade.available {
                if let Some(ref url) = lemonade.api_url {
                    let lemonade_models = fetch_lemonade_models(url);
                    add_lemonade_models(&mut self.models, lemonade_models);
                }
            }
        }

        if let Some(flm) = self
            .backends
            .iter()
            .find(|b| b.backend == Backend::FastFlowLm)
        {
            if flm.available {
                if let Some(ref url) = flm.api_url {
                    let flm_models = fetch_fastflowlm_models(url);
                    add_fastflowlm_models(&mut self.models, flm_models);
                }
            }
        }

        self.tree_nodes = build_tree(&self.models, &self.config);
        self.apply_filters();
    }

    // -- Theme --

    pub fn cycle_theme(&mut self) {
        self.theme = self.theme.next();
        self.theme.save();
    }
}

fn first_available(backends: &[DetectedBackend]) -> usize {
    backends.iter().position(|b| b.available).unwrap_or(0)
}

/// Build the source tree from discovered models.
fn build_tree(models: &[DiscoveredModel], config: &Config) -> Vec<TreeNode> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut nodes = Vec::new();

    // Built-in sources
    let builtins: Vec<(ModelSource, &str, Option<PathBuf>)> = vec![
        (ModelSource::LmStudio, "LM Studio", {
            let primary = home.join(".lmstudio").join("models");
            let cache = home.join(".cache").join("lm-studio").join("models");
            if primary.is_dir() {
                Some(primary)
            } else if cache.is_dir() {
                Some(cache)
            } else {
                Some(primary)
            }
        }),
        (
            ModelSource::LlamaCppCache,
            "llama.cpp",
            Some(home.join(".cache").join("llm-models")),
        ),
        (
            ModelSource::HfCache,
            "HF/MLX",
            Some(
                std::env::var("HF_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| home.join(".cache").join("huggingface").join("hub")),
            ),
        ),
        (
            ModelSource::LlmFit,
            "llmfit",
            Some(home.join(".cache").join("llmfit").join("models")),
        ),
        (ModelSource::Ollama, "Ollama", None),
        (ModelSource::Lemonade, "Lemonade", None),
        (ModelSource::FastFlowLm, "FastFlowLM", None),
    ];

    for (source, label, path) in builtins {
        let model_indices: Vec<usize> = models
            .iter()
            .enumerate()
            .filter(|(_, m)| m.source == source)
            .map(|(i, _)| i)
            .collect();
        let count = model_indices.len();

        let display = if let Some(ref p) = path {
            let short = shorten_path(p, &home);
            format!("{label} ({short})")
        } else {
            format!("{label} (API)")
        };

        nodes.push(TreeNode {
            label: display,
            path,
            source: Some(source),
            model_count: count,
            expanded: true,
            removable: false,
            model_indices,
        });
    }

    // Custom directories
    for dir in &config.extra_model_dirs {
        let model_indices: Vec<usize> = models
            .iter()
            .enumerate()
            .filter(|(_, m)| m.source == ModelSource::ExtraDir && m.path.starts_with(dir))
            .map(|(i, _)| i)
            .collect();
        let count = model_indices.len();
        let short = shorten_path(dir, &home);

        nodes.push(TreeNode {
            label: format!("{short}"),
            path: Some(dir.clone()),
            source: Some(ModelSource::ExtraDir),
            model_count: count,
            expanded: true,
            removable: true,
            model_indices,
        });
    }

    nodes
}

/// Compute filesystem completions for the current input.
fn compute_completions(input: &str) -> Vec<String> {
    if input.is_empty() {
        return Vec::new();
    }

    // Expand ~ to home dir for lookup
    let expanded = if input.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            let rest = input.strip_prefix("~/").unwrap_or(&input[1..]);
            home.join(rest).to_string_lossy().to_string()
        } else {
            input.to_string()
        }
    } else {
        input.to_string()
    };

    let path = std::path::Path::new(&expanded);

    // If input ends with /, list contents of that directory
    // Otherwise, treat parent as directory and filename prefix as filter
    let (dir, prefix) = if expanded.ends_with('/') || path.is_dir() && input.ends_with('/') {
        (path.to_path_buf(), String::new())
    } else {
        let parent = path.parent().unwrap_or(std::path::Path::new("/"));
        let prefix = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        (parent.to_path_buf(), prefix)
    };

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let home = dirs::home_dir();
    let prefix_lower = prefix.to_lowercase();

    let mut results: Vec<String> = entries
        .flatten()
        .filter(|e| {
            // Only show directories
            e.file_type().is_ok_and(|ft| ft.is_dir())
        })
        .filter(|e| {
            if prefix.is_empty() {
                true
            } else {
                e.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .starts_with(&prefix_lower)
            }
        })
        .filter(|e| {
            // Hide hidden dirs unless user typed a dot
            let name = e.file_name().to_string_lossy().to_string();
            !name.starts_with('.') || prefix.starts_with('.')
        })
        .map(|e| {
            let full = e.path();
            // Show with ~ if original input used ~
            if input.starts_with('~') {
                if let Some(ref h) = home {
                    if let Ok(stripped) = full.strip_prefix(h) {
                        return format!("~/{}", stripped.display());
                    }
                }
            }
            full.display().to_string()
        })
        .collect();

    results.sort();
    results.truncate(10); // Cap at 10 completions
    results
}

fn shorten_path(path: &PathBuf, home: &PathBuf) -> String {
    if let Ok(stripped) = path.strip_prefix(home) {
        format!("~/{}", stripped.display())
    } else {
        path.display().to_string()
    }
}
