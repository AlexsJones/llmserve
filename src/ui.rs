use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table};

use crate::app::{App, CONFIRM_FIELDS, ConfirmField, Focus, InputMode};
use crate::theme::ThemeColors;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let tc = app.theme.colors();

    let bg_style = Style::default().bg(tc.bg).fg(tc.fg);
    frame.render_widget(Block::default().style(bg_style), frame.area());

    let rows = Layout::vertical(vec![
        Constraint::Length(3), // header
        Constraint::Min(5),    // main area
        Constraint::Length(1), // status bar
    ])
    .split(frame.area());

    draw_header(frame, app, rows[0], &tc);

    // Build horizontal layout based on which panels are visible
    let mut constraints = Vec::new();
    if app.show_tree {
        constraints.push(Constraint::Length(app.tree_width));
    }
    constraints.push(Constraint::Min(30)); // table always present
    if app.show_serve {
        constraints.push(Constraint::Length(app.serve_width));
    }

    let main_cols = Layout::horizontal(constraints).split(rows[1]);

    let mut col = 0;
    if app.show_tree {
        draw_source_tree(frame, app, main_cols[col], &tc);
        col += 1;
    }
    draw_model_table(frame, app, main_cols[col], &tc);
    col += 1;
    if app.show_serve && col < main_cols.len() {
        draw_right_panel(frame, app, main_cols[col], &tc);
    }
    draw_status_bar(frame, app, rows[2], &tc);

    // Popups
    match app.input_mode {
        InputMode::BackendPopup => draw_backend_popup(frame, app, &tc),
        InputMode::ConfirmServe => draw_confirm_popup(frame, app, &tc),
        InputMode::StopPopup => draw_stop_popup(frame, app, &tc),
        InputMode::AddDir => draw_add_dir_popup(frame, app, &tc),
        InputMode::ServerExit => draw_server_exit_popup(frame, app, &tc),
        _ => {}
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect, tc: &ThemeColors) {
    let backend_label = app
        .active_backend()
        .map(|b| format!("{} [{}]", b.backend.label(), b.status_label()))
        .unwrap_or_else(|| "none".into());

    let server_info = if app.servers.is_empty() {
        String::new()
    } else {
        format!(" │ {} serving", app.servers.len())
    };

    let title = Line::from(vec![
        Span::styled(
            " llmserve ",
            Style::default().fg(tc.title).add_modifier(Modifier::BOLD),
        ),
        Span::styled("│ ", Style::default().fg(tc.muted)),
        Span::styled("Backend: ", Style::default().fg(tc.muted)),
        Span::styled(&backend_label, Style::default().fg(tc.accent)),
        Span::styled(
            format!(" │ {} models", app.models.len()),
            Style::default().fg(tc.muted),
        ),
        Span::styled(
            server_info,
            Style::default()
                .fg(tc.good)
                .add_modifier(if app.servers.is_empty() {
                    Modifier::empty()
                } else {
                    Modifier::BOLD
                }),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(tc.border))
        .style(Style::default().bg(tc.bg));

    frame.render_widget(Paragraph::new(title).block(block), area);
}

fn draw_source_tree(frame: &mut Frame, app: &App, area: Rect, tc: &ThemeColors) {
    let focused = app.focus == Focus::Tree && app.input_mode == InputMode::Normal;
    let border_color = if focused { tc.accent } else { tc.border };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(" Sources ")
        .title_style(Style::default().fg(tc.title).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(tc.bg));

    let inner = block.inner(area);
    let max_lines = inner.height as usize;

    let mut lines: Vec<Line> = Vec::new();

    for (i, node) in app.tree_nodes.iter().enumerate() {
        let is_cursor = focused && i == app.tree_cursor;
        let is_active = app.tree_source_filter.as_ref() == node.source.as_ref();

        let arrow = if node.expanded { "▼ " } else { "▶ " };
        let icon = if node.removable { "  " } else { "" };

        let label_style = if is_cursor {
            Style::default().bg(tc.highlight_bg).fg(tc.fg)
        } else if is_active {
            Style::default().fg(tc.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(tc.fg)
        };

        let count_str = format!(" ({})", node.model_count);
        let label_with_count = format!("{}{}", node.label, count_str);

        lines.push(Line::from(vec![
            Span::styled(
                if is_cursor { "> " } else { "  " },
                Style::default().fg(tc.accent),
            ),
            Span::styled(arrow, Style::default().fg(tc.muted)),
            Span::styled(icon, Style::default()),
            Span::styled(label_with_count, label_style),
        ]));

        // Show models under expanded nodes (truncated to fit)
        if node.expanded && lines.len() < max_lines {
            let remaining = max_lines.saturating_sub(lines.len());
            let show = node.model_indices.len().min(remaining.saturating_sub(1));
            for &mi in node.model_indices.iter().take(show) {
                let m = &app.models[mi];
                let serving = app.is_model_served(&m.name);
                let dot = if serving { "● " } else { "  " };
                let dot_color = if serving { tc.good } else { tc.muted };

                // Truncate name to fit panel
                let max_name = (inner.width as usize).saturating_sub(8);
                let truncated: String = m.name.chars().take(max_name).collect();

                lines.push(Line::from(vec![
                    Span::styled("      ", Style::default()),
                    Span::styled(dot, Style::default().fg(dot_color)),
                    Span::styled(truncated, Style::default().fg(tc.muted)),
                ]));
            }
            if node.model_indices.len() > show {
                lines.push(Line::from(vec![Span::styled(
                    format!("      +{} more", node.model_indices.len() - show),
                    Style::default().fg(tc.muted),
                )]));
            }
        }

        if lines.len() >= max_lines {
            break;
        }
    }

    // Add directory hint at bottom
    if lines.len() < max_lines {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "  [a] Add directory",
            Style::default().fg(tc.muted),
        )]));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_model_table(frame: &mut Frame, app: &mut App, area: Rect, tc: &ThemeColors) {
    let focused = app.focus == Focus::Table && app.input_mode == InputMode::Normal;
    let border_color = if focused { tc.accent } else { tc.border };

    // Title shows active filter info
    let title = if let Some(ref src) = app.tree_source_filter {
        format!(" Models [{src}] ")
    } else {
        " Models ".to_string()
    };

    // Search / filter bar integrated into title area
    let search_display = if app.input_mode == InputMode::Search {
        format!("/{}_", app.search_query)
    } else if !app.search_query.is_empty() {
        format!("/{}", app.search_query)
    } else {
        String::new()
    };

    let filter_info = format!(
        " {search_display} fmt:{} sort:{} ",
        app.format_filter.label(),
        app.sort_order.label()
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(title)
        .title_style(Style::default().fg(tc.title).add_modifier(Modifier::BOLD))
        .title_bottom(Line::from(filter_info).right_aligned())
        .style(Style::default().bg(tc.bg));

    let inner = block.inner(area);
    app.visible_rows = inner.height.saturating_sub(1) as usize;

    let header_cells = [
        "#",
        "Model Name",
        "Size",
        "Quant",
        "Params",
        "Fmt",
        "Status",
    ]
    .iter()
    .map(|h| Cell::from(*h).style(Style::default().fg(tc.accent).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1);

    let rows: Vec<Row> = app
        .filtered
        .iter()
        .enumerate()
        .skip(app.scroll_offset)
        .take(app.visible_rows)
        .map(|(display_idx, &model_idx)| {
            let m = &app.models[model_idx];
            let is_selected = focused && display_idx == app.selected;

            let style = if is_selected {
                Style::default().bg(tc.highlight_bg).fg(tc.fg)
            } else {
                Style::default().fg(tc.fg)
            };

            let vision = if m.mmproj.is_some() { " [V]" } else { "" };

            let srv = app.servers.iter().find(|s| s.model_name == m.name);
            let status = match srv {
                Some(s) if s.ready => format!(":{} ({})", s.port, s.backend.label()),
                Some(s) => format!(":{} loading…", s.port),
                None => String::new(),
            };
            let status_style = match srv {
                Some(s) if s.ready => Style::default().fg(tc.good),
                Some(_) => Style::default().fg(tc.warning),
                None => style,
            };

            Row::new(vec![
                Cell::from(format!("{}", display_idx + 1)),
                Cell::from(format!("{}{vision}", m.name)),
                Cell::from(m.size_display()),
                Cell::from(m.quant.as_deref().unwrap_or("-")),
                Cell::from(m.param_hint.as_deref().unwrap_or("-")),
                Cell::from(m.format.to_string()),
                Cell::from(status).style(status_style),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(4),
        Constraint::Min(18),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(4),
        Constraint::Length(18),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(Style::default().bg(tc.highlight_bg));

    frame.render_widget(table, area);
}

fn draw_right_panel(frame: &mut Frame, app: &mut App, area: Rect, tc: &ThemeColors) {
    // Split vertically: server cards on top, logs on bottom
    let server_card_height = if app.servers.is_empty() {
        0
    } else {
        // 3 lines per server + 2 border + 1 hint
        (app.servers.len() as u16 * 3 + 3).min(area.height / 2)
    };

    let chunks = Layout::vertical(vec![
        Constraint::Length(server_card_height),
        Constraint::Min(4),
    ])
    .split(area);

    if !app.servers.is_empty() {
        draw_server_cards(frame, app, chunks[0], tc);
    }
    draw_log_panel(frame, app, chunks[1], tc);
}

fn draw_server_cards(frame: &mut Frame, app: &App, area: Rect, tc: &ThemeColors) {
    let focused = app.focus == Focus::Serve && app.input_mode == InputMode::Normal;
    let title = format!(" Serving ({}) ", app.servers.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { tc.accent } else { tc.good }))
        .title(title)
        .title_style(Style::default().fg(tc.good).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(tc.bg));

    let inner_width = block.inner(area).width as usize;

    let mut lines: Vec<Line> = Vec::new();

    for s in &app.servers {
        let name: String = s
            .model_name
            .chars()
            .take(inner_width.saturating_sub(4))
            .collect();
        let (state, state_color) = if s.ready {
            ("●", tc.good)
        } else {
            ("◌", tc.warning)
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {state} "), Style::default().fg(state_color)),
            Span::styled(
                name,
                Style::default().fg(tc.fg).add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "   {} :{} │ {}",
                    s.backend.label(),
                    s.port,
                    s.uptime_display()
                ),
                Style::default().fg(tc.accent),
            ),
            Span::styled(
                if s.ready { "" } else { " │ loading…" },
                Style::default().fg(tc.warning),
            ),
        ]));
        lines.push(Line::from(vec![Span::styled(
            format!("   PID {} │ {}", s.pid, s.display_url()),
            Style::default().fg(tc.muted),
        )]));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_log_panel(frame: &mut Frame, app: &mut App, area: Rect, tc: &ThemeColors) {
    let focused = app.focus == Focus::Serve && app.input_mode == InputMode::Normal;
    let wrap_label = if app.log_wrap { "wrap:on" } else { "wrap:off" };

    let title = if app.log_scroll > 0 {
        format!(" Logs (scrolled ▲{}) ", app.log_scroll)
    } else {
        " Logs ".to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { tc.accent } else { tc.border }))
        .title(title)
        .title_style(Style::default().fg(tc.title).add_modifier(Modifier::BOLD))
        .title_bottom(
            Line::from(format!(" j/k:scroll [w]:{wrap_label} [C]:clear "))
                .right_aligned()
                .style(Style::default().fg(tc.muted)),
        )
        .style(Style::default().bg(tc.bg));

    let inner = block.inner(area);
    let visible = inner.height as usize;
    let inner_width = inner.width as usize;

    if inner_width == 0 || visible == 0 {
        frame.render_widget(block, area);
        return;
    }

    let all_logs = app.all_log_lines();

    // Build display lines — either truncated or wrapped
    let mut display_lines: Vec<(Style, String)> = Vec::new();

    for (_source, text) in &all_logs {
        let style = log_line_style(text, tc);

        if app.log_wrap && text.len() > inner_width {
            // Word wrap: break at width boundaries
            let chars: Vec<char> = text.chars().collect();
            let mut pos = 0;
            while pos < chars.len() {
                let end = (pos + inner_width).min(chars.len());
                let chunk: String = chars[pos..end].iter().collect();
                // Only first chunk gets the style; continuations are indented
                if pos == 0 {
                    display_lines.push((style, chunk));
                } else {
                    display_lines.push((style, format!(" {chunk}")));
                }
                pos = end;
            }
        } else {
            let truncated: String = text.chars().take(inner_width).collect();
            display_lines.push((style, truncated));
        }
    }

    // Pinned to the bottom unless the user scrolled up
    let max_scroll = display_lines.len().saturating_sub(visible);
    app.log_scroll = app.log_scroll.min(max_scroll);
    let skip = display_lines.len().saturating_sub(visible + app.log_scroll);

    let lines: Vec<Line> = display_lines
        .iter()
        .skip(skip)
        .take(visible)
        .map(|(style, text)| Line::from(vec![Span::styled(text, *style)]))
        .collect();

    if lines.is_empty() {
        let empty_lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  No output yet",
                Style::default().fg(tc.muted),
            )]),
        ];
        frame.render_widget(Paragraph::new(empty_lines).block(block), area);
    } else {
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }
}

fn log_line_style(text: &str, tc: &ThemeColors) -> Style {
    if text.contains("error") || text.contains("Error") || text.contains("FATAL") {
        Style::default().fg(tc.error)
    } else if text.contains("warn") || text.contains("Warn") {
        Style::default().fg(tc.warning)
    } else if text.starts_with("---") {
        Style::default().fg(tc.accent).add_modifier(Modifier::BOLD)
    } else if text.contains("─── live ───") {
        Style::default().fg(tc.good)
    } else {
        Style::default().fg(tc.muted)
    }
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect, tc: &ThemeColors) {
    let mode_text = match app.input_mode {
        InputMode::Normal => match app.focus {
            Focus::Tree => "SOURCES",
            Focus::Table => "NORMAL",
            Focus::Serve => "LOGS",
        },
        InputMode::Search => "SEARCH",
        InputMode::BackendPopup => "BACKEND",
        InputMode::ConfirmServe => "CONFIRM",
        InputMode::StopPopup => "STOP",
        InputMode::AddDir => "ADD DIR",
        InputMode::ServerExit => "EXITED",
    };

    let help = match app.input_mode {
        InputMode::Search => "type to filter │ Enter:confirm │ Esc:clear",
        InputMode::StopPopup => "j/k:select │ Enter:stop │ Esc:cancel",
        InputMode::ServerExit => "j/k:scroll │ r:restart │ Enter/Esc:dismiss",
        InputMode::ConfirmServe => {
            "j/k:field │ h/l/Space:change │ e:edit │ s:save default │ Enter:serve │ Esc:cancel"
        }
        InputMode::AddDir => "type path │ Tab:complete │ Enter:add │ Esc:cancel",
        InputMode::Normal if app.focus == Focus::Tree => {
            "j/k:nav │ Enter:filter │ Space:expand │ a:add │ x:rm │ S+←→:resize │ Tab:next │ q:quit"
        }
        InputMode::Normal if app.focus == Focus::Serve => {
            "w:wrap │ C:clear │ s:stop │ S:stop-all │ S+←→:resize │ 1:tree │ 3:logs │ Tab:next │ q:quit"
        }
        _ => {
            "Tab:next │ j/k:nav │ /:search │ Enter:serve │ s:stop │ 1:tree │ 3:logs │ S+←→:resize │ q:quit"
        }
    };

    let status_msg = app.status_message.as_deref().unwrap_or("");

    let line = Line::from(vec![
        Span::styled(
            format!(" {mode_text} "),
            Style::default()
                .bg(tc.status_bg)
                .fg(tc.status_fg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {help}"), Style::default().fg(tc.muted)),
        Span::styled(format!("  {status_msg}"), Style::default().fg(tc.info)),
    ]);

    frame.render_widget(Paragraph::new(line).style(Style::default().bg(tc.bg)), area);
}

fn draw_backend_popup(frame: &mut Frame, app: &App, tc: &ThemeColors) {
    let area = centered_rect(40, app.backends.len() as u16 + 4, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Select Backend ")
        .title_style(Style::default().fg(tc.title).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(tc.accent))
        .style(Style::default().bg(tc.bg));

    let items: Vec<Line> = app
        .backends
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let marker = if i == app.backend_popup_cursor {
                "> "
            } else {
                "  "
            };
            let status_color = if b.available { tc.good } else { tc.error };
            Line::from(vec![
                Span::styled(marker, Style::default().fg(tc.accent)),
                Span::styled(
                    format!("{:<12}", b.backend.label()),
                    Style::default().fg(if i == app.backend_popup_cursor {
                        tc.fg
                    } else {
                        tc.muted
                    }),
                ),
                Span::styled(
                    format!("[{}]", b.status_label()),
                    Style::default().fg(status_color),
                ),
            ])
        })
        .collect();

    frame.render_widget(
        Paragraph::new(items)
            .block(block)
            .alignment(Alignment::Left),
        area,
    );
}

fn draw_confirm_popup(frame: &mut Frame, app: &App, tc: &ThemeColors) {
    let area = centered_rect(62, 18, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Serve Model ")
        .title_style(Style::default().fg(tc.title).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(tc.accent))
        .style(Style::default().bg(tc.bg));

    let inner_width = block.inner(area).width as usize;
    let model_name = app.selected_model().map(|m| m.name.as_str()).unwrap_or("?");

    let backend = app.confirm_backend();
    let backend_label = backend.map(|b| b.backend.label()).unwrap_or("?");
    let backend_available = backend.is_some_and(|b| b.available);

    let backend_status = if !app.confirm_compatible() {
        let reason = app.confirm_incompatible_reason().unwrap_or("incompatible");
        Span::styled(format!(" [{reason}]"), Style::default().fg(tc.error))
    } else if !backend_available {
        Span::styled(" [not found]", Style::default().fg(tc.error))
    } else if app.confirm_already_serving() {
        Span::styled(" [already serving]", Style::default().fg(tc.warning))
    } else {
        Span::styled(" [ready]", Style::default().fg(tc.good))
    };

    let preset = &app.confirm_preset;
    let selected = app.confirm_selected_field();

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("    Model:      ", Style::default().fg(tc.muted)),
            Span::styled(model_name, Style::default().fg(tc.fg)),
        ]),
        Line::from(""),
    ];

    let auto = |v: Option<String>| v.unwrap_or_else(|| "auto".into());

    for field in CONFIRM_FIELDS {
        let is_sel = field == selected;
        let editing = is_sel && app.confirm_editing;

        let (label, mut value, hint) = match field {
            ConfirmField::Backend => (
                "Backend",
                format!("< {backend_label} >"),
                String::new(), // status span appended below
            ),
            ConfirmField::Host => ("Host", preset.host.clone(), String::new()),
            ConfirmField::Port => ("Port", app.confirm_port_input.clone(), String::new()),
            ConfirmField::CtxSize => (
                "Context",
                format!(
                    "[{}] {}",
                    if preset.use_ctx_size { "x" } else { " " },
                    preset.ctx_size
                ),
                match app.confirm_model_ctx {
                    Some(max) if u64::from(preset.ctx_size) > max => {
                        format!("(exceeds model max {max}!)")
                    }
                    Some(max) => format!("(model max {max})"),
                    None => "(Space toggles)".into(),
                },
            ),
            ConfirmField::FlashAttn => (
                "Flash attn",
                if preset.flash_attn { "on" } else { "off" }.into(),
                String::new(),
            ),
            ConfirmField::GpuLayers => (
                "GPU layers",
                auto(preset.gpu_layers.map(|v| v.to_string())),
                String::new(),
            ),
            ConfirmField::Threads => (
                "Threads",
                auto(preset.threads.map(|v| v.to_string())),
                String::new(),
            ),
            ConfirmField::BatchSize => (
                "Batch",
                auto(preset.batch_size.map(|v| v.to_string())),
                String::new(),
            ),
            ConfirmField::ExtraArgs => (
                "Extra args",
                if preset.extra_args.is_empty() {
                    "(none)".into()
                } else {
                    preset.extra_args.join(" ")
                },
                String::new(),
            ),
        };

        if editing {
            value = format!("{}_", app.confirm_edit_buf);
        }
        let max_val = inner_width.saturating_sub(18 + hint.len());
        let value: String = value.chars().take(max_val).collect();

        let marker = if is_sel { "  > " } else { "    " };
        let value_style = if editing {
            Style::default()
                .fg(tc.fg)
                .add_modifier(Modifier::UNDERLINED)
        } else if is_sel {
            Style::default().fg(tc.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(tc.fg)
        };

        let mut spans = vec![
            Span::styled(marker, Style::default().fg(tc.accent)),
            Span::styled(format!("{label:<12}"), Style::default().fg(tc.muted)),
            Span::styled(value, value_style),
        ];
        if field == ConfirmField::Backend {
            spans.push(backend_status.clone());
        } else if is_sel && !hint.is_empty() {
            spans.push(Span::styled(
                format!("  {hint}"),
                Style::default().fg(tc.muted),
            ));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(""));
    if app.confirm_editing {
        lines.push(Line::from(vec![Span::styled(
            "  Enter:done │ Esc:cancel edit  (empty = auto)",
            Style::default().fg(tc.warning),
        )]));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "  j/k:field │ h/l/Space:change │ e:edit │ s:save default",
            Style::default().fg(tc.warning),
        )]));
        lines.push(Line::from(vec![Span::styled(
            "  Enter:serve │ Esc:cancel",
            Style::default().fg(tc.warning),
        )]));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_stop_popup(frame: &mut Frame, app: &App, tc: &ThemeColors) {
    let item_count = app.servers.len() + 1;
    let area = centered_rect(55, item_count as u16 + 4, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Stop Server ")
        .title_style(Style::default().fg(tc.title).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(tc.accent))
        .style(Style::default().bg(tc.bg));

    let mut items: Vec<Line> = app
        .servers
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let marker = if i == app.stop_popup_cursor {
                "> "
            } else {
                "  "
            };
            let fg = if i == app.stop_popup_cursor {
                tc.fg
            } else {
                tc.muted
            };
            Line::from(vec![
                Span::styled(marker, Style::default().fg(tc.accent)),
                Span::styled(
                    format!("{:<6}", s.backend.label()),
                    Style::default().fg(tc.accent),
                ),
                Span::styled(
                    format!(" {} (:{}, {})", s.model_name, s.port, s.uptime_display()),
                    Style::default().fg(fg),
                ),
            ])
        })
        .collect();

    let stop_all_idx = app.servers.len();
    let marker = if app.stop_popup_cursor == stop_all_idx {
        "> "
    } else {
        "  "
    };
    items.push(Line::from(vec![
        Span::styled(marker, Style::default().fg(tc.accent)),
        Span::styled(
            "Stop All",
            Style::default()
                .fg(if app.stop_popup_cursor == stop_all_idx {
                    tc.error
                } else {
                    tc.warning
                })
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    frame.render_widget(
        Paragraph::new(items)
            .block(block)
            .alignment(Alignment::Left),
        area,
    );
}

fn draw_add_dir_popup(frame: &mut Frame, app: &App, tc: &ThemeColors) {
    let comp_count = app.add_dir_completions.len();
    let height = 6 + comp_count.min(10) as u16;
    let area = centered_rect(65, height, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Add Model Directory ")
        .title_style(Style::default().fg(tc.title).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(tc.accent))
        .style(Style::default().bg(tc.bg));

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Path: ", Style::default().fg(tc.muted)),
            Span::styled(
                format!("{}_", app.add_dir_input),
                Style::default()
                    .fg(tc.fg)
                    .add_modifier(Modifier::UNDERLINED),
            ),
        ]),
    ];

    // Show completions
    if !app.add_dir_completions.is_empty() {
        lines.push(Line::from(""));
        for (i, comp) in app.add_dir_completions.iter().enumerate() {
            let is_selected = i == app.add_dir_completion_idx;
            let marker = if is_selected { "  > " } else { "    " };
            let style = if is_selected {
                Style::default().fg(tc.accent)
            } else {
                Style::default().fg(tc.muted)
            };
            lines.push(Line::from(vec![
                Span::styled(marker, Style::default().fg(tc.accent)),
                Span::styled(comp, style),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Tab:complete │ Enter:add │ Esc:cancel",
        Style::default().fg(tc.warning),
    )]));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_server_exit_popup(frame: &mut Frame, app: &mut App, tc: &ThemeColors) {
    let Some(info) = app.current_exit_popup() else {
        return;
    };

    let frame_area = frame.area();
    let width = frame_area.width.saturating_sub(8).clamp(40, 80);
    // 4 header lines + 2 borders, plus the log tail
    let wanted = info.log_lines.len().max(1) as u16 + 6;
    let height = wanted.min(frame_area.height.saturating_sub(4)).max(8);
    let area = centered_rect(width, height, frame_area);
    frame.render_widget(Clear, area);

    let title = if app.exit_popups.len() > 1 {
        format!(" Server Exited (1 of {}) ", app.exit_popups.len())
    } else {
        " Server Exited ".to_string()
    };

    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(tc.error).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(tc.error))
        .title_bottom(
            Line::from(" j/k:scroll │ r:restart │ Enter/Esc:dismiss ")
                .right_aligned()
                .style(Style::default().fg(tc.muted)),
        )
        .style(Style::default().bg(tc.bg));

    let inner = block.inner(area);
    let inner_width = inner.width as usize;

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Model:   ", Style::default().fg(tc.muted)),
            Span::styled(
                info.model.name.clone(),
                Style::default().fg(tc.fg).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Backend: ", Style::default().fg(tc.muted)),
            Span::styled(
                format!("{} (port {})", info.backend_label, info.port),
                Style::default().fg(tc.accent),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Status:  ", Style::default().fg(tc.muted)),
            Span::styled(info.message.clone(), Style::default().fg(tc.error)),
        ]),
        Line::from(vec![Span::styled(
            "  ── last output ──",
            Style::default().fg(tc.muted),
        )]),
    ];

    // Log tail fills the remaining height; scroll offset counts up from the bottom
    let log_visible = (inner.height as usize).saturating_sub(lines.len());
    let max_scroll = info.log_lines.len().saturating_sub(log_visible);
    let scroll = app.exit_popup_scroll.min(max_scroll);
    let skip = info.log_lines.len().saturating_sub(log_visible + scroll);

    if info.log_lines.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  (no output captured)",
            Style::default().fg(tc.muted),
        )]));
    } else {
        for text in info.log_lines.iter().skip(skip).take(log_visible) {
            let style = log_line_style(text, tc);
            let truncated: String = text.chars().take(inner_width.saturating_sub(2)).collect();
            lines.push(Line::from(vec![Span::styled(
                format!("  {truncated}"),
                style,
            )]));
        }
    }

    app.exit_popup_scroll = scroll;

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
