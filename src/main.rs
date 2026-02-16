mod app;
mod config;
mod state;
mod tmux;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::App;
use config::Config;

fn main() -> io::Result<()> {
    // Check tmux availability before entering raw mode
    if let Err(msg) = tmux::check_available() {
        eprintln!("Error: {}", msg);
        std::process::exit(1);
    }

    let config = Config::load();

    // Install panic hook to restore terminal on crash
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, &config);

    // Always restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let jump_target = result?;

    // Jump to pane after TUI cleanup
    if let Some(ref pane_id) = jump_target {
        if let Err(e) = tmux::jump_to_pane(pane_id) {
            eprintln!("Warning: {}", e);
        }
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config: &Config,
) -> io::Result<Option<String>> {
    let mut app = App::new(config);
    let mut last_refresh = std::time::Instant::now();

    loop {
        terminal.draw(|f| ui::draw(f, &app, config))?;

        // Auto-refresh every 1 second
        if last_refresh.elapsed() >= Duration::from_secs(1) {
            app.refresh();
            app.update_preview();
            last_refresh = std::time::Instant::now();
        }

        // Poll for events with 200ms timeout
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Up => app.move_up(),
                        KeyCode::Down => app.move_down(),
                        KeyCode::Enter => app.jump(),
                        KeyCode::Esc => {
                            if app.filter.is_empty() {
                                app.quit();
                            } else {
                                app.clear_filter();
                            }
                        }
                        KeyCode::Backspace => app.delete_filter_char(),
                        KeyCode::Char(c) => app.add_filter_char(c),
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(app.jump_target)
}
