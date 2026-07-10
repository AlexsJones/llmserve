use std::io;

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use llmserve::app::App;
use llmserve::events::handle_events;
use llmserve::ui;

const HELP: &str = "\
llmserve — TUI for discovering local LLM models and serving them

Usage: llmserve [OPTIONS]

Options:
  -h, --help     Print this help
  -V, --version  Print version

Keys (in the TUI):
  Tab        cycle panel focus          /   search models
  j/k        navigate                   f   cycle format filter
  Enter      serve selected model       o   cycle sort order
  s / S      stop server / stop all     b   select backend
  a / x      add / remove model dir     r   rescan models & backends
  1 / 3      toggle sources / logs      t   cycle theme
  q          quit

Config: ~/.config/llmserve/config.toml";

/// Restore the terminal even if we panic, so the user's shell isn't left in
/// raw mode on the alternate screen with no cursor.
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original(info);
    }));
}

fn main() -> io::Result<()> {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "-V" | "--version" => {
                println!("llmserve {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            other => {
                eprintln!("Unknown option: {other}\nTry 'llmserve --help'");
                std::process::exit(2);
            }
        }
    }

    install_panic_hook();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // Note: mouse capture is deliberately not enabled — it would prevent
    // normal terminal text selection (copying URLs or log lines).
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }

    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = App::new();

    loop {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        handle_events(&mut app)?;

        if app.should_quit {
            if !app.servers.is_empty() {
                app.stop_all_servers();
            }
            break;
        }
    }

    Ok(())
}
