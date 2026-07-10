use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::time::Duration;

use crate::app::{App, Focus, InputMode};

pub fn handle_events(app: &mut App) -> std::io::Result<bool> {
    app.tick();

    if event::poll(Duration::from_millis(50))?
        && let Event::Key(key) = event::read()?
    {
        if key.kind != KeyEventKind::Press {
            return Ok(false);
        }
        match app.input_mode {
            InputMode::Normal => handle_normal(app, key),
            InputMode::Search => handle_search(app, key),
            InputMode::BackendPopup => handle_backend_popup(app, key),
            InputMode::ConfirmServe => handle_confirm(app, key),
            InputMode::StopPopup => handle_stop_popup(app, key),
            InputMode::AddDir => handle_add_dir(app, key),
            InputMode::ServerExit => handle_server_exit_popup(app, key),
        }
        return Ok(true);
    }
    Ok(false)
}

fn handle_normal(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            // If tree source filter is active and we're in the tree, clear it first
            if app.focus == Focus::Tree && app.tree_source_filter.is_some() {
                app.tree_source_filter = None;
                app.apply_filters();
            } else {
                app.should_quit = true;
            }
        }

        // Tab switches focus between tree and table
        KeyCode::Tab => app.toggle_focus(),

        // Navigation — works in both tree and table based on focus
        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
        KeyCode::Home | KeyCode::Char('g') => app.home(),
        KeyCode::End | KeyCode::Char('G') => app.end(),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => app.half_page_up(),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => app.half_page_down(),

        // Enter: in tree = select/filter source, in table = serve
        KeyCode::Enter => {
            if app.focus == Focus::Tree {
                app.tree_select_source();
            } else {
                app.open_confirm_serve();
            }
        }

        // Tree-specific: expand/collapse
        KeyCode::Char(' ') if app.focus == Focus::Tree => app.tree_toggle_expand(),

        // Add directory (works from either panel)
        KeyCode::Char('a') => app.open_add_dir(),

        // Remove custom directory (tree only)
        KeyCode::Char('x') if app.focus == Focus::Tree => app.tree_remove_dir(),

        // Search
        KeyCode::Char('/') => app.enter_search(),

        // Filters / sort
        KeyCode::Char('f') => app.cycle_format_filter(),
        KeyCode::Char('o') => app.cycle_sort(),

        // Backend
        KeyCode::Char('b') => app.open_backend_popup(),

        // Stop server
        KeyCode::Char('s') => app.open_stop_popup(),
        KeyCode::Char('S') => app.stop_all_servers(),

        // Refresh
        KeyCode::Char('r') => app.refresh(),

        // Toggle panels: 1=sources, 3=logs
        KeyCode::Char('1') => app.toggle_tree(),
        KeyCode::Char('3') => app.toggle_serve_panel(),

        // Clear dead logs
        KeyCode::Char('C') => app.clear_dead_logs(),

        // Toggle log word wrap
        KeyCode::Char('w') => app.toggle_log_wrap(),

        // Resize focused pane: Shift+Left / Shift+Right
        KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => app.shrink_focused_pane(),
        KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => app.grow_focused_pane(),

        // Theme
        KeyCode::Char('t') => app.cycle_theme(),

        _ => {}
    }
}

fn handle_search(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.clear_search(),
        KeyCode::Enter => app.exit_search(),
        KeyCode::Backspace => app.search_pop(),
        KeyCode::Char(c) => app.search_push(c),
        _ => {}
    }
}

fn handle_backend_popup(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.cancel_popup(),
        KeyCode::Down | KeyCode::Char('j') => app.backend_popup_down(),
        KeyCode::Up | KeyCode::Char('k') => app.backend_popup_up(),
        KeyCode::Enter => app.select_backend(),
        _ => {}
    }
}

fn handle_confirm(app: &mut App, key: KeyEvent) {
    if app.confirm_editing {
        match key.code {
            KeyCode::Enter | KeyCode::Tab => app.confirm_commit_edit(),
            KeyCode::Esc => app.confirm_cancel_edit(),
            KeyCode::Backspace => app.confirm_edit_pop(),
            KeyCode::Char(c) => app.confirm_edit_push(c),
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Enter | KeyCode::Char('y') => app.do_serve(),
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => app.cancel_popup(),
        KeyCode::Down | KeyCode::Char('j') => app.confirm_field_down(),
        KeyCode::Up | KeyCode::Char('k') => app.confirm_field_up(),
        KeyCode::Left | KeyCode::Char('h') => app.confirm_field_left(),
        KeyCode::Right | KeyCode::Char('l') => app.confirm_field_right(),
        KeyCode::Char(' ') => app.confirm_toggle(),
        KeyCode::Tab | KeyCode::Char('e') | KeyCode::Char('p') => app.confirm_begin_edit(),
        KeyCode::Char('s') => app.confirm_save_preset(),
        _ => {}
    }
}

fn handle_stop_popup(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.cancel_popup(),
        KeyCode::Down | KeyCode::Char('j') => app.stop_popup_down(),
        KeyCode::Up | KeyCode::Char('k') => app.stop_popup_up(),
        KeyCode::Enter => app.confirm_stop(),
        _ => {}
    }
}

fn handle_server_exit_popup(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => app.dismiss_exit_popup(),
        KeyCode::Down | KeyCode::Char('j') => app.exit_popup_scroll_down(),
        KeyCode::Up | KeyCode::Char('k') => app.exit_popup_scroll_up(),
        _ => {}
    }
}

fn handle_add_dir(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => app.confirm_add_dir(),
        KeyCode::Esc => app.cancel_add_dir(),
        KeyCode::Backspace => app.add_dir_pop(),
        KeyCode::Tab => app.add_dir_accept_completion(),
        KeyCode::Down => app.add_dir_next_completion(),
        KeyCode::Up => app.add_dir_prev_completion(),
        KeyCode::Char(c) => app.add_dir_push(c),
        _ => {}
    }
}
