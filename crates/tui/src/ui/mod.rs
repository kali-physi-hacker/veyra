//! Rendering: the shell (header, navigation, footer, overlays, toasts) and page dispatch.
mod apps;
mod audit;
mod cleanup;
mod duplicates;
mod files;
mod insights;
mod overview;
mod storage;
mod system;

use crate::{
    app::{App, Overlay, Page, ToastKind},
    theme::Theme,
    widgets::{self, card, card_accent},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Clear, List, ListItem, Paragraph, Wrap},
};

pub const SIDEBAR_WIDTH: u16 = 24;
pub const COMPACT_WIDTH: u16 = 100;

/// Below this width the sidebar collapses into a tab strip.
pub fn compact(width: u16) -> bool {
    width < COMPACT_WIDTH
}
pub fn page_color(theme: &Theme, page: Page) -> Color {
    match page {
        Page::Overview => theme.accent,
        Page::Insights => theme.amber,
        Page::Storage => theme.teal,
        Page::Files => theme.blue,
        Page::Apps => theme.rose,
        Page::Duplicates => theme.lavender,
        Page::Cleanup => theme.amber,
        Page::System => theme.cyan,
        Page::Audit => theme.muted,
    }
}
pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    app.size = (area.width, area.height);
    let theme = app.theme;
    frame.render_widget(Block::new().style(theme.base()), area);
    if area.width < 40 || area.height < 10 {
        frame.render_widget(
            Paragraph::new("Stratum needs at least 40×10 cells.")
                .style(theme.muted())
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(6),
        Constraint::Length(1),
    ])
    .areas(area);
    render_header(frame, app, header);
    let content = if compact(area.width) {
        let [tabs, content] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(4)]).areas(body);
        render_tabs(frame, app, tabs);
        content
    } else {
        let [side, content] =
            Layout::horizontal([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(40)])
                .areas(body);
        render_sidebar(frame, app, side);
        content
    };
    render_content(frame, app, content);
    render_footer(frame, app, footer);
    match app.overlay {
        Overlay::None => {}
        Overlay::Help => render_help(frame, app, area),
        Overlay::Scan => render_scan_dialog(frame, app, area),
        Overlay::Progress => render_progress(frame, app, area),
        Overlay::Roots => render_roots(frame, app, area),
        Overlay::Operations => render_operations(frame, app, area),
    }
    render_toast(frame, app, area);
}
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let mut left = vec![
        Span::raw(" "),
        Span::styled("◆", theme.bold(theme.accent)),
        Span::raw(" "),
        Span::styled("STRATUM", theme.strong()),
        Span::raw("  "),
        Span::styled("Local machine intelligence", theme.muted()),
    ];
    let status = status_spans(app);
    let status_width = status
        .iter()
        .map(|s| widgets::width(&s.content))
        .sum::<usize>() as u16
        + 1;
    // The scope only appears when it fits between the wordmark and the status.
    let scope_budget = (area.width as usize)
        .saturating_sub(status_width as usize + 42)
        .min(48);
    if let Some(scan) = app.overview.as_ref().and_then(|o| o.coverage.first())
        && scope_budget >= 18
    {
        left.push(Span::styled("  ·  ", theme.faint()));
        left.push(Span::styled(
            widgets::truncate_middle(&widgets::short_path(&scan.root), scope_budget - 12),
            theme.text(),
        ));
        left.push(Span::raw(" "));
        left.push(widgets::badge(
            &theme,
            widgets::freshness_label(&scan.freshness),
            widgets::freshness_color(&theme, &scan.freshness),
        ));
        let more = app.overview.as_ref().map_or(0, |o| o.coverage.len()) - 1;
        if more > 0 {
            left.push(Span::styled(format!("  +{more} more"), theme.muted()));
        }
    }
    let left_width = area.width.saturating_sub(status_width + 1);
    frame.render_widget(Line::from(left), Rect::new(area.x, area.y, left_width, 1));
    frame.render_widget(
        Line::from(status).right_aligned(),
        Rect::new(area.x + left_width, area.y, status_width, 1),
    );
    frame.render_widget(
        Line::styled("─".repeat(area.width as usize), theme.border()),
        Rect::new(area.x, area.y + 1, area.width, 1),
    );
}
fn status_spans(app: &App) -> Vec<Span<'static>> {
    let theme = app.theme;
    let mut spans = Vec::new();
    if app.scan.active() {
        spans.push(Span::styled(
            "●",
            theme.color(widgets::pulse(&theme, app.tick)),
        ));
        spans.push(Span::styled(
            format!(
                " Indexing {} entries · {} · {}{}",
                widgets::count(app.scan.entries),
                widgets::bytes(app.scan.bytes),
                widgets::clock(app.scan.elapsed()),
                if app.scan.paused { " · paused" } else { "" }
            ),
            theme.text(),
        ));
    } else if app.busy {
        spans.push(Span::styled(widgets::spinner(app.tick), theme.accent()));
        spans.push(Span::styled(format!(" {}", app.status), theme.text()));
    } else if app.loading() {
        spans.push(Span::styled(widgets::spinner(app.tick), theme.accent()));
        spans.push(Span::styled(" Reading index", theme.muted()));
    } else {
        spans.push(Span::styled("●", theme.color(theme.teal)));
        spans.push(Span::styled(" Ready · local only", theme.muted()));
    }
    spans.push(Span::raw(" "));
    spans
}
fn render_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let buf = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        if let Some(cell) = buf.cell_mut((area.right() - 1, y)) {
            cell.set_symbol("│").set_style(theme.border());
        }
    }
    let inner = Rect::new(area.x, area.y, area.width - 1, area.height);
    let mut y = inner.y + 1;
    let mut group = "";
    for page in Page::ALL {
        if y + 1 >= inner.bottom().saturating_sub(3) {
            break;
        }
        if page.group() != group {
            if !group.is_empty() {
                y += 1;
            }
            group = page.group();
            buf.set_string(inner.x + 2, y, group.to_uppercase(), theme.eyebrow());
            y += 1;
            if y >= inner.bottom() {
                break;
            }
        }
        let active = app.page == page;
        let color = page_color(&theme, page);
        if active {
            buf.set_style(
                Rect::new(inner.x, y, inner.width, 1),
                Style::new().bg(theme.raised),
            );
            buf.set_string(inner.x, y, "▌", theme.color(color).bg(theme.raised));
        }
        let title_style = if active {
            theme.strong().bg(theme.raised)
        } else {
            theme.text()
        };
        let number = page.number().to_string();
        let glyph_style = if active {
            theme.bold(color).bg(theme.raised)
        } else {
            theme.color(color)
        };
        buf.set_string(inner.x + 2, y, page.glyph(), glyph_style);
        buf.set_string(
            inner.x + 4,
            y,
            widgets::fit(page.title(), inner.width.saturating_sub(7) as usize),
            title_style,
        );
        buf.set_string(
            inner.right().saturating_sub(2),
            y,
            number,
            if active {
                theme.muted().bg(theme.raised)
            } else {
                theme.faint()
            },
        );
        y += 1;
    }
    if inner.height >= 8 {
        buf.set_string(
            inner.x + 2,
            inner.bottom() - 3,
            "●",
            theme.color(theme.teal),
        );
        buf.set_string(
            inner.x + 4,
            inner.bottom() - 3,
            "On this device",
            theme.text(),
        );
        buf.set_string(
            inner.x + 2,
            inner.bottom() - 2,
            widgets::fit("local · no account", inner.width.saturating_sub(3) as usize),
            theme.faint(),
        );
    }
}
fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let mut spans = vec![Span::raw(" ")];
    for page in Page::ALL {
        let color = page_color(&theme, page);
        if app.page == page {
            spans.push(Span::styled(
                format!(" {} {} ", page.glyph(), page.title()),
                theme.strong().bg(theme.raised),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {}", page.glyph()),
                theme.color(color),
            ));
            spans.push(Span::styled(format!("{} ", page.number()), theme.faint()));
        }
    }
    frame.render_widget(Line::from(spans), Rect::new(area.x, area.y, area.width, 1));
    frame.render_widget(
        Line::styled("─".repeat(area.width as usize), theme.border()),
        Rect::new(area.x, area.y + 1, area.width, 1),
    );
}
fn render_content(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let inner = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    let color = page_color(&theme, app.page);
    frame.render_widget(
        Line::from(vec![
            Span::styled(app.page.glyph(), theme.bold(color)),
            Span::raw(" "),
            Span::styled(app.page.title(), theme.strong()),
        ]),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let body_top = if inner.height >= 12 {
        frame.render_widget(
            Line::styled(
                widgets::truncate_end(app.page.subtitle(), inner.width as usize),
                theme.muted(),
            ),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
        3
    } else {
        2
    };
    let body = Rect::new(
        inner.x,
        inner.y + body_top,
        inner.width,
        inner.height.saturating_sub(body_top),
    );
    if body.height == 0 {
        return;
    }
    match app.page {
        Page::Overview => overview::render(frame, app, body),
        Page::Insights => insights::render(frame, app, body),
        Page::Storage => storage::render(frame, app, body),
        Page::Files => files::render(frame, app, body),
        Page::Apps => apps::render(frame, app, body),
        Page::Duplicates => duplicates::render(frame, app, body),
        Page::Cleanup => cleanup::render(frame, app, body),
        Page::System => system::render(frame, app, body),
        Page::Audit => audit::render(frame, app, body),
    }
}
fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let hints = app.hints();
    let line = widgets::hints(&theme, &hints);
    let version = format!("Stratum {} ", env!("CARGO_PKG_VERSION"));
    let hints_width = line.width() as u16;
    if hints_width + version.len() as u16 + 2 <= area.width {
        frame.render_widget(
            Line::styled(version, theme.faint()).right_aligned(),
            Rect::new(area.x + hints_width, area.y, area.width - hints_width, 1),
        );
    }
    frame.render_widget(line, area);
}
/// A centered card explaining that nothing is available yet.
pub fn empty_state(frame: &mut Frame, theme: &Theme, area: Rect, title: &str, detail: &str) {
    let width = area.width.min(64);
    let lines = wrap_text(detail, width.saturating_sub(4) as usize);
    let height = (lines.len() as u16 + 4).min(area.height);
    let rect = widgets::centered(area, width, height);
    let mut text = vec![
        Line::styled(title.to_string(), theme.strong()),
        Line::raw(""),
    ];
    text.extend(lines.into_iter().map(|l| Line::styled(l, theme.muted())));
    frame.render_widget(
        Paragraph::new(Text::from(text)).block(card(theme, "")),
        rect,
    );
}
pub fn loading(frame: &mut Frame, app: &App, area: Rect, what: &str) {
    let theme = app.theme;
    frame.render_widget(
        Line::from(vec![
            Span::styled(widgets::spinner(app.tick), theme.accent()),
            Span::styled(format!(" {what}"), theme.muted()),
        ]),
        Rect::new(area.x, area.y, area.width, 1),
    );
}
/// Greedy word wrap that respects display width.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let word_width = widgets::width(word);
            if !line.is_empty() && widgets::width(&line) + 1 + word_width > width {
                lines.push(std::mem::take(&mut line));
            }
            if word_width > width {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                let mut chunk = String::new();
                for c in word.chars() {
                    if widgets::width(&chunk)
                        + unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
                        > width
                    {
                        lines.push(std::mem::take(&mut chunk));
                    }
                    chunk.push(c);
                }
                line = chunk;
                continue;
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    while lines.last().is_some_and(String::is_empty) && lines.len() > 1 {
        lines.pop();
    }
    lines
}
fn modal(frame: &mut Frame, area: Rect, width: u16, height: u16) -> Rect {
    let rect = widgets::centered(
        area,
        width.min(area.width.saturating_sub(2)),
        height.min(area.height.saturating_sub(1)),
    );
    frame.render_widget(Clear, rect);
    rect
}
fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let entries: [(&str, &str); 22] = [
        ("1-9  Tab  ⇧Tab", "switch pages"),
        ("↑ ↓  j k", "move selection"),
        ("PgUp PgDn  g G", "page · first · last"),
        ("Enter  →  l", "open folder / investigate"),
        ("⌫  ←  h", "parent folder"),
        ("b", "previous folder"),
        ("L", "choose an indexed location"),
        ("f", "largest files in this folder"),
        ("m", "cycle file views"),
        ("/", "filter names, e.g. *.zip"),
        ("] [", "next / previous page of results"),
        ("c", "review cleanup candidates here"),
        ("x  space", "toggle a cleanup candidate"),
        ("a  n", "select all on page · clear selection"),
        ("p", "create an immutable cleanup plan"),
        ("u", "restore a quarantined operation"),
        ("o", "previous operations · sort processes"),
        ("v", "verify duplicate content"),
        ("s", "scan a folder (read-only)"),
        ("r", "refresh from the local index"),
        ("Esc", "close dialogs"),
        ("q  ^C", "quit"),
    ];
    let height = (entries.len() as u16 + 5).min(area.height.saturating_sub(2));
    let rect = modal(frame, area, 68, height);
    let mut lines = vec![
        Line::styled(
            "Nothing scans, moves or deletes without an explicit key press.",
            theme.muted(),
        ),
        Line::raw(""),
    ];
    for (keys, what) in entries {
        lines.push(Line::from(vec![
            Span::styled(format!("{keys:<16}"), theme.bold(theme.accent)),
            Span::styled(what, theme.text()),
        ]));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: true })
            .block(card_accent(&theme, "Keyboard", theme.accent)),
        rect,
    );
}
fn render_scan_dialog(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let suggestions = app.scan_suggestions();
    let height = (suggestions.len() as u16 + 13).min(area.height.saturating_sub(2));
    let rect = modal(frame, area, 70, height);
    let width = rect.width.saturating_sub(4) as usize;
    let mut lines = vec![
        Line::styled("Start focused. Expand when you need to.", theme.strong()),
        Line::styled(
            "Scanning reads metadata into a private local index. It does not authorize cleanup.",
            theme.muted(),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Folder  ", theme.eyebrow()),
            Span::styled(
                widgets::truncate_middle(&app.scan_input, width.saturating_sub(9)),
                theme.text(),
            ),
            Span::styled("█", theme.color(widgets::pulse(&theme, app.tick))),
        ]),
        Line::raw(""),
        Line::styled("Suggestions · Tab to cycle", theme.eyebrow()),
    ];
    for (index, (path, label)) in suggestions.iter().enumerate() {
        let chosen = app.scan_suggestion == Some(index) || app.scan_input == *path;
        let marker = if chosen { "▸ " } else { "  " };
        let style = if chosen {
            theme.bold(theme.teal)
        } else {
            theme.text()
        };
        lines.push(Line::from(vec![
            Span::styled(marker, theme.accent()),
            Span::styled(
                widgets::fit(&widgets::short_path(path), width.saturating_sub(22)),
                style,
            ),
            Span::styled(format!("  {label}"), theme.muted()),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Overlapping roots are rejected. macOS may restrict some folders; warnings are recorded.",
        theme.faint(),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(card_accent(&theme, "Scan a location", theme.teal)),
        rect,
    );
}
fn render_progress(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let rect = modal(frame, area, 72, 9);
    let width = rect.width.saturating_sub(4) as usize;
    let title = format!(
        "{} {}",
        if app.scan.paused {
            "Paused"
        } else {
            "Scanning"
        },
        widgets::truncate_middle(&widgets::short_path(&app.scan.root), 40)
    );
    let mut sweep = widgets::sweep(&theme, app.tick, width, theme.teal);
    if app.scan.paused {
        sweep = vec![Span::styled("━".repeat(width), theme.faint())];
    }
    let lines = vec![
        Line::from(vec![
            Span::styled(widgets::spinner(app.tick), theme.accent()),
            Span::styled(
                format!(
                    " {} entries · {} discovered · {} elapsed",
                    widgets::count(app.scan.entries),
                    widgets::bytes(app.scan.bytes),
                    widgets::clock(app.scan.elapsed())
                ),
                theme.text(),
            ),
        ]),
        Line::from(sweep),
        Line::styled(
            widgets::truncate_middle(&widgets::short_path(&app.scan.last_path), width),
            theme.muted(),
        ),
        Line::from(vec![
            Span::styled(
                format!("{} warnings", app.scan.warnings),
                if app.scan.warnings > 0 {
                    theme.color(theme.amber)
                } else {
                    theme.faint()
                },
            ),
            Span::styled(
                "  ·  results publish only when the scan finishes",
                theme.faint(),
            ),
        ]),
        Line::raw(""),
        widgets::hints(
            &theme,
            &[
                ("space", if app.scan.paused { "resume" } else { "pause" }),
                ("c", "cancel"),
                ("Esc", "hide"),
            ],
        ),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(card_accent(&theme, &title, theme.teal)),
        rect,
    );
}
fn render_roots(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let height = (app.roots.len() as u16 + 4).clamp(5, area.height.saturating_sub(2));
    let rect = modal(frame, area, 64, height);
    if app.roots.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "No indexed locations yet. Press s to scan a folder.",
                theme.muted(),
            ))
            .block(card(&theme, "Indexed locations")),
            rect,
        );
        return;
    }
    let scans = app
        .overview
        .as_ref()
        .map(|o| o.coverage.clone())
        .unwrap_or_default();
    let items: Vec<ListItem> = app
        .roots
        .iter()
        .map(|root| {
            let scan = scans.iter().find(|s| s.root == *root);
            let mut spans = vec![Span::styled(
                widgets::fit(
                    &widgets::short_path(root),
                    rect.width.saturating_sub(26) as usize,
                ),
                theme.text(),
            )];
            if let Some(scan) = scan {
                spans.push(Span::raw(" "));
                spans.push(widgets::badge(
                    &theme,
                    widgets::freshness_label(&scan.freshness),
                    widgets::freshness_color(&theme, &scan.freshness),
                ));
                spans.push(Span::styled(
                    format!(" {}", widgets::bytes(scan.logical_bytes)),
                    theme.muted(),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let mut state = app.roots_nav.list_state();
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected())
            .highlight_symbol("▌ ")
            .block(card(&theme, "Indexed locations")),
        rect,
        &mut state,
    );
    app.roots_nav.offset = state.offset();
}
fn render_operations(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let height = (app.operations.len() as u16 + 4).clamp(5, area.height.saturating_sub(2));
    let rect = modal(frame, area, 72, height);
    if app.operations.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "No quarantine operations have been recorded in this state directory.",
                theme.muted(),
            ))
            .wrap(Wrap { trim: true })
            .block(card(&theme, "Previous operations")),
            rect,
        );
        return;
    }
    let items: Vec<ListItem> = app
        .operations
        .iter()
        .map(|operation| {
            let restored = operation
                .items
                .iter()
                .filter(|i| i.status == "restored")
                .count();
            ListItem::new(Line::from(vec![
                Span::styled(
                    widgets::status_glyph(&operation.status),
                    theme.color(if operation.status.contains("fail") {
                        theme.rose
                    } else {
                        theme.teal
                    }),
                ),
                Span::styled(
                    format!(" {:<12}", widgets::humanize(&operation.status)),
                    theme.text(),
                ),
                Span::styled(
                    format!(
                        "{} files · {} restored · {}",
                        operation.items.len(),
                        restored,
                        widgets::age(operation.created_at)
                    ),
                    theme.muted(),
                ),
                Span::styled(
                    format!("  {}", widgets::truncate_end(&operation.id, 13)),
                    theme.faint(),
                ),
            ]))
        })
        .collect();
    let mut state = app.operations_nav.list_state();
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected())
            .highlight_symbol("▌ ")
            .block(card(
                &theme,
                "Previous operations · Enter to inspect or restore",
            )),
        rect,
        &mut state,
    );
    app.operations_nav.offset = state.offset();
}
fn render_toast(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let Some(toast) = &app.toast else {
        return;
    };
    let (glyph, color) = match toast.kind {
        ToastKind::Info => ("●", theme.accent),
        ToastKind::Success => ("✓", theme.teal),
        ToastKind::Warning => ("⚠", theme.amber),
        ToastKind::Error => ("✗", theme.rose),
    };
    let max_width = area.width.saturating_sub(4).min(72) as usize;
    let lines = wrap_text(&toast.text, max_width.saturating_sub(4));
    let width = lines.iter().map(|l| widgets::width(l)).max().unwrap_or(0) as u16 + 6;
    let height = lines.len() as u16 + 2;
    let rect = Rect::new(
        area.right().saturating_sub(width + 1),
        area.bottom().saturating_sub(height + 1),
        width.min(area.width),
        height.min(area.height),
    );
    frame.render_widget(Clear, rect);
    let text: Vec<Line> = lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            Line::from(vec![
                Span::styled(if i == 0 { glyph } else { " " }, theme.bold(color)),
                Span::raw(" "),
                Span::styled(line, theme.text()),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(Text::from(text)).block(
            Block::bordered()
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(theme.color(color))
                .style(Style::new().bg(theme.bg)),
        ),
        rect,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_collapses_below_one_hundred_columns() {
        assert!(compact(80));
        assert!(compact(99));
        assert!(!compact(100));
        assert!(!compact(160));
    }
    #[test]
    fn wrapping_respects_width_and_long_words() {
        let lines = wrap_text("the quick brown fox jumps over the lazy dog", 12);
        assert!(lines.iter().all(|l| widgets::width(l) <= 12), "{lines:?}");
        assert_eq!(
            lines.join(" "),
            "the quick brown fox jumps over the lazy dog"
        );
        let long = wrap_text("abcdefghijklmnopqrstuvwxyz", 10);
        assert_eq!(long, vec!["abcdefghij", "klmnopqrst", "uvwxyz"]);
        assert_eq!(wrap_text("a\n\nb", 10), vec!["a", "", "b"]);
    }
}
