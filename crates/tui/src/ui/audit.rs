//! Audit: the durable local record of what Stratum did.
use super::{empty_state, wrap_text};
use crate::{
    app::{App, PAGE_SIZE},
    widgets::{self, card, card_accent},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span, Text},
    widgets::{List, ListItem, Paragraph, Wrap},
};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    if app.audit.is_empty() {
        if !app.waiting() {
            empty_state(
                frame,
                &theme,
                area,
                "Your actions leave a record",
                "Scans, plans, moves and restores will appear here with their exact detail.",
            );
        }
        return;
    }
    let [list_area, detail_area, footer] = Layout::vertical([
        Constraint::Min(4),
        Constraint::Length(7),
        Constraint::Length(1),
    ])
    .areas(area);
    let block = card_accent(&theme, "Timeline", theme.muted);
    let width = block.inner(list_area).width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = app
        .audit
        .iter()
        .map(|record| {
            let color = match record.action.as_str() {
                a if a.contains("fail") || a.contains("error") => theme.rose,
                a if a.contains("restore") => theme.teal,
                a if a.contains("cleanup") || a.contains("quarantine") => theme.amber,
                a if a.contains("scan") => theme.blue,
                _ => theme.muted,
            };
            ListItem::new(Line::from(vec![
                Span::styled("● ", theme.color(color)),
                Span::styled(
                    format!("{:<9}", widgets::age(record.timestamp)),
                    theme.muted(),
                ),
                Span::styled(
                    widgets::fit(&widgets::humanize(&record.action), 28),
                    theme.text(),
                ),
                Span::styled(
                    widgets::truncate_middle(&record.resource_id, width.saturating_sub(40)),
                    theme.faint(),
                ),
            ]))
        })
        .collect();
    let mut state = app.audit_nav.list_state();
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected())
            .highlight_symbol("▌ ")
            .block(block),
        list_area,
        &mut state,
    );
    app.audit_nav.offset = state.offset();
    let detail_block = card(&theme, "Recorded detail");
    let width = detail_block.inner(detail_area).width as usize;
    let mut lines = Vec::new();
    if let Some(record) = app
        .audit_nav
        .index(app.audit.len())
        .and_then(|i| app.audit.get(i))
    {
        lines.push(Line::from(vec![
            Span::styled(widgets::humanize(&record.action), theme.strong()),
            Span::styled(
                format!("  ·  {}  ·  #{}", record.resource_id, record.id),
                theme.muted(),
            ),
        ]));
        lines.extend(
            wrap_text(&record.detail, width)
                .into_iter()
                .take(4)
                .map(|l| Line::styled(l, theme.text())),
        );
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(detail_block),
        detail_area,
    );
    let page = app.audit_offset / u64::from(PAGE_SIZE) + 1;
    frame.render_widget(
        Line::styled(
            format!(
                "Page {page} · {} records · newest first{}",
                app.audit.len(),
                if app.audit.len() as u32 >= PAGE_SIZE {
                    " · ] older"
                } else {
                    ""
                }
            ),
            theme.faint(),
        ),
        footer,
    );
}
