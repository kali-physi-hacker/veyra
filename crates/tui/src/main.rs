use clap::Parser;
use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Paragraph, Tabs, Wrap},
};
use stratum_engine::{Engine, domain::*};
#[derive(Parser)]
struct Options {
    #[arg(long, env = "STRATUM_DATA_DIR")]
    data_dir: Option<std::path::PathBuf>,
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse();
    let engine = Engine::open(stratum_engine::load_config(None, options.data_dir)?)?;
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, engine);
    ratatui::restore();
    result.map_err(Into::into)
}
fn run(terminal: &mut ratatui::DefaultTerminal, engine: std::sync::Arc<Engine>) -> Result<()> {
    let pages = [
        "Overview",
        "Largest files",
        "Directories",
        "Insights",
        "Processes",
    ];
    let mut page = 0;
    let mut content = String::new();
    let mut refresh = true;
    let mut scroll = 0u16;
    loop {
        if refresh {
            content = match page {
                0 => {
                    let categories = engine.categories()?;
                    let scans = engine.scans()?;
                    format!(
                        "LOCAL ONLY · Persistent index\n\n{}\n\n{}",
                        scans
                            .iter()
                            .take(5)
                            .map(|s| format!(
                                "{} · {} · {} entries · {} warnings · {}",
                                s.root, s.status, s.entries, s.warnings, s.freshness
                            ))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        categories
                            .iter()
                            .map(|c| format!("{:24} {:>15} bytes", c.category, c.logical_bytes))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                }
                1 | 2 => engine
                    .files(&FileQuery {
                        kind: Some(if page == 1 { "file" } else { "directory" }.into()),
                        limit: 100,
                        ..Default::default()
                    })?
                    .items
                    .iter()
                    .map(|e| format!("{:>15}  {}", e.logical_bytes, e.path))
                    .collect::<Vec<_>>()
                    .join("\n"),
                3 => engine
                    .insights()?
                    .iter()
                    .map(|i| {
                        format!(
                            "{}\n{}\nConfidence {:.0}% · risk {}\n{}\n",
                            i.title,
                            i.description,
                            i.confidence * 100.0,
                            i.risk,
                            i.evidence
                                .iter()
                                .map(|e| format!("  {}: {}", e.mechanism, e.detail))
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => engine
                    .system()
                    .processes
                    .iter()
                    .take(100)
                    .map(|p| {
                        format!(
                            "{:>7} {:>6.1}% {:>12} bytes  {}",
                            p.pid, p.cpu_percent, p.memory_bytes, p.name
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            refresh = false;
        }
        terminal.draw(|f|{let areas=Layout::vertical([Constraint::Length(3),Constraint::Min(1),Constraint::Length(1)]).split(f.area());f.render_widget(Tabs::new(pages).select(page).highlight_style(Style::default().fg(Color::LightCyan)).block(Block::bordered().title("STRATUM · machine intelligence")),areas[0]);f.render_widget(Paragraph::new(content.as_str()).wrap(Wrap{trim:false}).scroll((scroll,0)).block(Block::bordered().title(pages[page])),areas[1]);f.render_widget(Paragraph::new("←/→ tabs · ↑/↓ scroll · r refresh · q quit · cleanup requires CLI/API plan approval"),areas[2]);}).map_err(Error::io)?;
        if event::poll(std::time::Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
        {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Right => {
                    page = (page + 1) % pages.len();
                    refresh = true;
                    scroll = 0;
                }
                KeyCode::Left => {
                    page = (page + pages.len() - 1) % pages.len();
                    refresh = true;
                    scroll = 0;
                }
                KeyCode::Char('r') => refresh = true,
                KeyCode::Down => scroll = scroll.saturating_add(1),
                KeyCode::Up => scroll = scroll.saturating_sub(1),
                _ => {}
            }
        }
    }
}
