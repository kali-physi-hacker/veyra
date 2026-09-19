//! Duplicates: the last verified report and its groups.
use super::empty_state;
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
    let [status_row, body] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(4)]).areas(area);
    if app.duplicate_running {
        let mut spans = vec![
            Span::styled(widgets::spinner(app.tick), theme.accent()),
            Span::styled(
                " Verifying content locally · size → sample → full BLAKE3  ",
                theme.text(),
            ),
        ];
        spans.extend(widgets::sweep(&theme, app.tick, 24, theme.lavender));
        frame.render_widget(Line::from(spans), status_row);
    } else {
        frame.render_widget(
            Line::from(vec![
                Span::styled(" v ", theme.strong().bg(theme.raised)),
                Span::styled(" Verify duplicate content", theme.bold(theme.lavender)),
                Span::styled(
                    widgets::truncate_end(
                        "  ·  size → sample → full BLAKE3 · never chooses which copy to remove",
                        status_row.width.saturating_sub(30) as usize,
                    ),
                    theme.faint(),
                ),
            ]),
            status_row,
        );
    }
    let Some(report) = app.duplicates.clone() else {
        if !app.loading() && !app.duplicate_running {
            empty_state(
                frame,
                &theme,
                body,
                "Verify before deciding",
                "Content analysis runs locally and never chooses which copy should be removed. Press v to hash indexed files.",
            );
        }
        return;
    };
    let [summary, groups_area] =
        Layout::vertical([Constraint::Length(5), Constraint::Min(4)]).areas(body);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(vec![
                Span::styled(
                    format!("{} duplicate groups", widgets::count(report.group_count)),
                    theme.strong(),
                ),
                Span::styled(
                    format!(
                        "  ·  {} files fully hashed · {} warnings · observed {}{}",
                        widgets::count(report.files_hashed),
                        report.warnings.len(),
                        widgets::age(report.analyzed_at),
                        if report.cancelled { " · cancelled early" } else { "" }
                    ),
                    theme.muted(),
                ),
            ]),
            Line::styled(
                "Observations from the last analysis, not a live guarantee. Sparse files and shared blocks affect physical savings. No copy is selected for deletion.",
                theme.faint(),
            ),
        ]))
        .wrap(Wrap { trim: true })
        .block(card_accent(&theme, "Last report", theme.lavender)),
        summary,
    );
    if report.groups.is_empty() {
        empty_state(
            frame,
            &theme,
            groups_area,
            "No groups in this report",
            "Run verification after indexing files. Cancellation and permission warnings may limit the report.",
        );
        return;
    }
    let wide = groups_area.width >= 96;
    let (list_area, detail_area) = if wide {
        let [list, detail] =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                .spacing(1)
                .areas(groups_area);
        (list, detail)
    } else {
        let [list, detail] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(groups_area);
        (list, detail)
    };
    let block = card(&theme, "Groups");
    let width = block.inner(list_area).width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = report
        .groups
        .iter()
        .map(|group| {
            let name = group
                .files
                .first()
                .map_or_else(|| "Duplicate group".into(), |p| widgets::file_name(p));
            ListItem::new(Text::from(vec![
                Line::from(vec![
                    Span::styled(
                        widgets::fit(&name, width.saturating_sub(12)),
                        theme.strong(),
                    ),
                    Span::raw(" "),
                    widgets::badge(
                        &theme,
                        &format!("{} copies", group.file_count),
                        theme.lavender,
                    ),
                ]),
                Line::styled(
                    format!(
                        "{} each · {} logically redundant",
                        widgets::bytes(group.file_size),
                        widgets::bytes(group.reclaimable_size)
                    ),
                    theme.muted(),
                ),
            ]))
        })
        .collect();
    let mut state = app.duplicates_nav.list_state();
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected())
            .highlight_symbol("▌ ")
            .block(block),
        list_area,
        &mut state,
    );
    app.duplicates_nav.offset = state.offset();
    let page = app.duplicates_offset / u64::from(PAGE_SIZE) + 1;
    let detail_block = card(&theme, &format!("Locations · page {page}"));
    let width = detail_block.inner(detail_area).width as usize;
    let mut lines = Vec::new();
    if let Some(group) = app
        .duplicates_nav
        .index(report.groups.len())
        .and_then(|i| report.groups.get(i))
    {
        for path in &group.files {
            lines.push(Line::from(vec![
                Span::styled("• ", theme.accent()),
                Span::styled(
                    widgets::truncate_middle(&widgets::short_path(path), width.saturating_sub(2)),
                    theme.text(),
                ),
            ]));
        }
        if group.file_count > group.files.len() as u64 {
            lines.push(Line::styled(
                format!("{} paths shown of {}", group.files.len(), group.file_count),
                theme.muted(),
            ));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(group.verification.clone(), theme.faint()));
        lines.push(Line::styled(
            format!("hash {}", widgets::truncate_end(&group.hash, 16)),
            theme.faint(),
        ));
    }
    if !report.warnings.is_empty() {
        lines.push(Line::raw(""));
        for warning in report.warnings.iter().take(5) {
            lines.push(Line::styled(
                format!(
                    "⚠ {}: {}",
                    warning.mechanism,
                    widgets::truncate_middle(&warning.detail, width.saturating_sub(4))
                ),
                theme.color(theme.amber),
            ));
        }
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(detail_block),
        detail_area,
    );
}
