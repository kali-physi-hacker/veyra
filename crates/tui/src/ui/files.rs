//! Files: largest entries, largest folders and this week's changes as a table.
use super::empty_state;
use crate::{
    app::{App, FilesMode, PAGE_SIZE},
    widgets::{self, card_accent},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Cell, Row, Table},
};
use stratum_engine::domain::*;

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let [tabs_row, filter_row, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(1),
    ])
    .areas(area);
    let mut spans = Vec::new();
    for mode in FilesMode::ALL {
        if mode == app.files_mode {
            spans.push(Span::styled(
                format!(" {} ", mode.label()),
                theme.strong().bg(theme.raised),
            ));
        } else {
            spans.push(Span::styled(format!(" {} ", mode.label()), theme.muted()));
        }
        spans.push(Span::raw(" "));
    }
    if !app.path.is_empty() {
        spans.push(Span::styled("in ", theme.faint()));
        spans.push(Span::styled(
            widgets::truncate_middle(&widgets::short_path(&app.path), 40),
            theme.color(theme.teal),
        ));
    }
    frame.render_widget(Line::from(spans), tabs_row);
    let filter_line = if app.filter_editing {
        Line::from(vec![
            Span::styled("⌕ ", theme.accent()),
            Span::styled(app.filter_draft.clone(), theme.text()),
            Span::styled("█", theme.color(widgets::pulse(&theme, app.tick))),
            Span::styled("  Enter applies · Esc cancels · e.g. *.zip", theme.faint()),
        ])
    } else if app.filter.is_empty() {
        Line::from(vec![
            Span::styled("⌕ ", theme.faint()),
            Span::styled("press / to filter names", theme.faint()),
        ])
    } else {
        Line::from(vec![
            Span::styled("⌕ ", theme.accent()),
            Span::styled(app.filter.clone(), theme.text()),
            Span::styled("  (/ to change)", theme.faint()),
        ])
    };
    frame.render_widget(filter_line, filter_row);
    if app.files.is_empty() && !app.loading() {
        empty_state(
            frame,
            &theme,
            body,
            "No matching indexed entries",
            "Try another view, filter or location. Files outside your scan scope are not included.",
        );
    } else {
        let header = Row::new(
            ["NAME", "SIZE", "ALLOCATED", "MODIFIED", "CATEGORY"]
                .into_iter()
                .map(|h| Cell::from(Span::styled(h, theme.eyebrow()))),
        )
        .bottom_margin(0);
        let name_width = body.width.saturating_sub(58) as usize;
        let rows = app.files.iter().map(|entry| {
            let name = if entry.kind == EntryKind::Directory {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            Row::new(vec![
                Cell::from(Line::from(vec![
                    Span::styled(
                        widgets::kind_glyph(&entry.kind),
                        theme.color(if entry.kind == EntryKind::Directory {
                            theme.teal
                        } else {
                            theme.blue
                        }),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        widgets::truncate_middle(&name, name_width.max(8)),
                        theme.text(),
                    ),
                ])),
                Cell::from(Span::styled(
                    widgets::bytes(entry.logical_bytes),
                    theme.text(),
                )),
                Cell::from(Span::styled(
                    widgets::bytes(entry.allocated_bytes),
                    theme.muted(),
                )),
                Cell::from(Span::styled(
                    entry
                        .modified_at
                        .map_or_else(|| "unknown".into(), widgets::age),
                    theme.muted(),
                )),
                Cell::from(Span::styled(
                    widgets::humanize(&entry.category),
                    theme.color(theme.lavender),
                )),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Min(20),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(18),
            ],
        )
        .header(header)
        .column_spacing(2)
        .row_highlight_style(theme.selected())
        .highlight_symbol("▌ ")
        .block(card_accent(&theme, "Entries", theme.blue));
        let mut state = app.files_nav.table_state();
        frame.render_stateful_widget(table, body, &mut state);
        app.files_nav.offset = state.offset();
    }
    let page = app.files_offset / u64::from(PAGE_SIZE) + 1;
    frame.render_widget(
        Line::from(vec![
            Span::styled(
                format!("Page {page} · {} rows", app.files.len()),
                theme.muted(),
            ),
            Span::styled(
                if app.files_has_more {
                    "  ·  ] next page"
                } else {
                    "  ·  end of results"
                },
                theme.faint(),
            ),
            Span::styled(
                if app.files_offset > 0 {
                    "  ·  [ previous page"
                } else {
                    ""
                },
                theme.faint(),
            ),
            Span::styled(
                "   ▸ folder  · file  ⇢ link · Enter opens the folder in Storage",
                theme.faint(),
            ),
        ]),
        footer,
    );
}
