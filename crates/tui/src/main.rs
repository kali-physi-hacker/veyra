//! Stratum terminal interface: a keyboard-driven view of the local machine index.
mod app;
mod theme;
mod ui;
mod widgets;
mod work;

use app::App;
use clap::Parser;
use crossterm::event::{self, Event};
use std::time::Duration;
use stratum_engine::Engine;
use theme::Theme;

#[derive(Parser)]
#[command(
    name = "stratum-tui",
    about = "Stratum in the terminal: explore, understand and review your machine's storage."
)]
struct Options {
    /// Dedicated private state directory (defaults to ~/.local/share/stratum).
    #[arg(long, env = "STRATUM_DATA_DIR")]
    data_dir: Option<std::path::PathBuf>,
    /// Optional configuration file.
    #[arg(long)]
    config: Option<std::path::PathBuf>,
    /// Color palette: `truecolor` (default) or `ansi` for terminals limited to sixteen colors.
    #[arg(long, value_enum, default_value = "truecolor")]
    palette: Palette,
}
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Palette {
    Truecolor,
    Ansi,
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse();
    let engine = Engine::open(stratum_engine::load_config(
        options.config.as_deref(),
        options.data_dir,
    )?)?;
    let theme = match options.palette {
        Palette::Truecolor => Theme::truecolor(),
        Palette::Ansi => Theme::ansi(),
    };
    // ratatui's init installs a panic hook that restores the terminal before
    // the panic message prints; restore() below covers the normal exit path.
    let mut terminal =
        ratatui::try_init().map_err(|e| format!("Stratum needs an interactive terminal: {e}"))?;
    let result = run(&mut terminal, App::new(engine, theme));
    ratatui::restore();
    result.map_err(Into::into)
}
fn run(terminal: &mut ratatui::DefaultTerminal, mut app: App) -> std::io::Result<()> {
    let idle = Duration::from_millis(100);
    loop {
        app.receive();
        if app.dirty || app.animating() {
            terminal.draw(|frame| ui::render(frame, &mut app))?;
            app.dirty = false;
        }
        if event::poll(idle)? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Resize(width, height) => app.resize(width, height),
                _ => {}
            }
        } else {
            app.tick();
        }
        if app.should_quit {
            return Ok(());
        }
    }
}
