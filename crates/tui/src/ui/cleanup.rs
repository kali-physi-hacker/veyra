//! Cleanup: choose exact files, review an immutable plan, authorize, inspect the outcome.
use super::{empty_state, wrap_text};
use crate::{
    app::{App, CleanupPhase, CleanupRow, PAGE_SIZE},
    widgets::{self, card_accent},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span, Text},
    widgets::{List, ListItem, Paragraph, Wrap},
};
use stratum_engine::domain::*;

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let [stepper, notice, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(4),
    ])
    .areas(area);
    let phase = app.cleanup_phase();
    let mut spans = Vec::new();
    for (index, (step, label)) in [
        (CleanupPhase::Select, "① Choose files"),
        (CleanupPhase::Review, "② Review exact plan"),
        (CleanupPhase::Outcome, "③ Inspect outcome"),
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            spans.push(Span::styled("  ▸  ", theme.faint()));
        }
        spans.push(Span::styled(
            label,
            if step == phase {
                theme.bold(theme.amber)
            } else {
                theme.muted()
            },
        ));
    }
    frame.render_widget(Line::from(spans), stepper);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("⚠ ", theme.color(theme.amber)),
            Span::styled(
                "Quarantine is reversible storage, not recovered capacity.",
                theme.bold(theme.amber),
            ),
            Span::styled(
                " Files move to Stratum's private storage on the same filesystem. Stop active builds first.",
                theme.muted(),
            ),
        ]))
        .wrap(Wrap { trim: true }),
        notice,
    );
    match phase {
        CleanupPhase::Select => render_selection(frame, app, body),
        CleanupPhase::Review => render_plan(frame, app, body),
        CleanupPhase::Outcome => render_outcome(frame, app, body),
    }
}
/// Before a folder is opened: every folder the cleanup rules recognise, largest first, straight
/// from the index.
fn render_folders(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let [bar, list_area, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(area);
    let selected_bytes: u64 = app.selected.values().sum();
    let total: u64 = app.locations.iter().map(|l| l.logical_bytes).sum();
    frame.render_widget(
        Line::from(vec![
            Span::styled(format!("{} folders", app.locations.len()), theme.strong()),
            Span::styled(format!(" · {} in the index", widgets::bytes(total)), theme.muted()),
            Span::styled(
                format!("   {} selected", app.selected.len()),
                if app.selected.is_empty() {
                    theme.faint()
                } else {
                    theme.bold(theme.teal)
                },
            ),
            Span::styled(format!(" · {}", widgets::bytes(selected_bytes)), theme.faint()),
        ]),
        bar,
    );
    if app.locations.is_empty() {
        if !app.waiting() {
            empty_state(
                frame,
                &theme,
                list_area,
                "No recognised cleanup folders in the index",
                "Candidate rules recognise Cargo build output beside a Cargo.toml, npm's package cache and Cargo's registry cache. Scan a location that holds them; a large file on its own is not a candidate.",
            );
        }
    } else {
        let block = card_accent(&theme, "Cleanup folders · largest first", theme.amber);
        let width = block.inner(list_area).width.saturating_sub(2) as usize;
        let largest = app.locations.first().map_or(1, |l| l.logical_bytes.max(1));
        let cells = 10;
        let kind_width = 21;
        let path_width = width.saturating_sub(kind_width + cells + 14);
        let items: Vec<ListItem> = app
            .locations
            .iter()
            .map(|location| {
                let color = if location.category == "developer_build_artifact" {
                    theme.accent
                } else {
                    theme.teal
                };
                let mut spans = vec![
                    Span::styled(widgets::fit(folder_kind(location), kind_width), theme.color(color)),
                    Span::raw(" "),
                    Span::styled(
                        widgets::fit(
                            &widgets::truncate_middle(&widgets::short_path(&location.path), path_width),
                            path_width,
                        ),
                        theme.text(),
                    ),
                    Span::styled(widgets::pad_left(&widgets::bytes(location.logical_bytes), 11), theme.strong()),
                    Span::raw(" "),
                ];
                spans.extend(widgets::bar(&theme, location.logical_bytes as f64 / largest as f64, cells, color));
                ListItem::new(Line::from(spans))
            })
            .collect();
        let mut state = app.locations_nav.list_state();
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(theme.selected())
                .highlight_symbol("▌ ")
                .block(block),
            list_area,
            &mut state,
        );
        app.locations_nav.offset = state.offset();
    }
    let reason = app
        .locations_nav
        .index(app.locations.len())
        .and_then(|i| app.locations.get(i))
        .map(|l| l.reason.clone())
        .unwrap_or_else(|| "Open a folder to choose exact files; nothing is preselected.".into());
    frame.render_widget(
        Line::from(vec![
            Span::styled("⏎ open  ", theme.muted()),
            Span::styled(
                widgets::truncate_end(&reason, footer.width.saturating_sub(10) as usize),
                theme.faint(),
            ),
        ]),
        footer,
    );
}
fn folder_kind(location: &CleanupLocation) -> &'static str {
    match location.category.as_str() {
        "developer_build_artifact" => "Cargo build output",
        _ if location.path.ends_with("/_cacache") => "npm package cache",
        _ => "Cargo registry cache",
    }
}
fn render_selection(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.cleanup_scope.is_none() {
        return render_folders(frame, app, area);
    }
    let theme = app.theme;
    let [bar, list_area, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(area);
    let selected_bytes: u64 = app.selected.values().sum();
    let mut spans = vec![
        Span::styled(
            format!("{} selected", app.selected.len()),
            if app.selected.is_empty() {
                theme.muted()
            } else {
                theme.bold(theme.teal)
            },
        ),
        Span::styled(
            format!(
                " · {} for reversible quarantine",
                widgets::bytes(selected_bytes)
            ),
            theme.muted(),
        ),
    ];
    if let Some(scope) = &app.cleanup_scope {
        spans.push(Span::styled("   scope ", theme.faint()));
        spans.push(Span::styled(
            widgets::truncate_middle(&widgets::short_path(scope), 40),
            theme.color(theme.teal),
        ));
        spans.push(Span::styled("  (0 = all folders)", theme.faint()));
    }
    frame.render_widget(Line::from(spans), bar);
    if app.candidates.is_empty() {
        if !app.waiting() {
            empty_state(
                frame,
                &theme,
                list_area,
                if app.cleanup_has_more {
                    "No candidates on this page"
                } else {
                    "No candidate files here"
                },
                if app.cleanup_has_more {
                    "Press ] to continue through this folder."
                } else {
                    "Only Cargo build output and downloaded package caches are candidates; a large file is not one on its own. Press 0 for every recognised folder."
                },
            );
        }
    } else {
        let block = card_accent(&theme, "Candidates · nothing is preselected", theme.amber);
        let width = block.inner(list_area).width.saturating_sub(2) as usize;
        let name_width = (width / 3).clamp(12, 36);
        let path_width = width.saturating_sub(name_width + 18);
        let items: Vec<ListItem> = app
            .cleanup_rows
            .iter()
            .map(|row| match row {
                CleanupRow::Header { category, count } => ListItem::new(Line::from(vec![
                    Span::styled(widgets::humanize(category).to_uppercase(), theme.eyebrow()),
                    Span::styled(format!("  {count} on this page  "), theme.muted()),
                    widgets::badge(&theme, "moderate risk", theme.amber),
                    Span::raw(" "),
                    widgets::badge(&theme, "undo supported", theme.teal),
                ])),
                CleanupRow::Candidate(index) => {
                    let candidate = &app.candidates[*index];
                    let checked = app.selected.contains_key(&candidate.path);
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            if checked { "[x] " } else { "[ ] " },
                            if checked {
                                theme.bold(theme.teal)
                            } else {
                                theme.muted()
                            },
                        ),
                        Span::styled(
                            widgets::fit(&widgets::file_name(&candidate.path), name_width),
                            if checked {
                                theme.strong()
                            } else {
                                theme.text()
                            },
                        ),
                        Span::raw(" "),
                        Span::styled(
                            widgets::fit(
                                &widgets::truncate_middle(
                                    &widgets::short_path(&candidate.path),
                                    path_width,
                                ),
                                path_width,
                            ),
                            theme.faint(),
                        ),
                        Span::styled(
                            widgets::pad_left(&widgets::bytes(candidate.size), 11),
                            theme.color(theme.accent),
                        ),
                    ]))
                }
            })
            .collect();
        let mut state = app.cleanup_nav.list_state();
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(theme.selected())
                .highlight_symbol("▌ ")
                .block(block),
            list_area,
            &mut state,
        );
        app.cleanup_nav.offset = state.offset();
    }
    let page = app.cleanup_offset / u64::from(PAGE_SIZE) + 1;
    let reason = app
        .cleanup_nav
        .index(app.cleanup_rows.len())
        .and_then(|i| match &app.cleanup_rows[i] {
            CleanupRow::Candidate(index) => app.candidates.get(*index).map(|c| c.reason.clone()),
            CleanupRow::Header { .. } => None,
        })
        .unwrap_or_else(|| {
            "Selection is for reversible quarantine, not recovered capacity.".into()
        });
    frame.render_widget(
        Line::from(vec![
            Span::styled(format!("Page {page}"), theme.muted()),
            Span::styled(
                if app.cleanup_has_more {
                    "  ] next  "
                } else {
                    "  "
                },
                theme.faint(),
            ),
            Span::styled(
                widgets::truncate_end(&reason, footer.width.saturating_sub(16) as usize),
                theme.faint(),
            ),
        ]),
        footer,
    );
}
fn render_plan(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let Some(plan) = &app.plan else {
        return;
    };
    let block = card_accent(&theme, "Immutable plan · no files have moved", theme.amber);
    let inner = block.inner(area);
    let width = inner.width as usize;
    let remaining = plan.expires_at.saturating_sub(now()).max(0);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(widgets::bytes(plan.total_bytes), theme.strong()),
            Span::styled(
                format!(
                    " across {} file{} · {} risk · {}",
                    plan.items.len(),
                    if plan.items.len() == 1 { "" } else { "s" },
                    plan.risk,
                    if remaining == 0 {
                        "expired".to_string()
                    } else {
                        format!("expires in {} min", remaining / 60)
                    }
                ),
                if remaining == 0 {
                    theme.color(theme.rose)
                } else {
                    theme.muted()
                },
            ),
        ]),
        Line::raw(""),
        Line::styled("Exact files in this plan", theme.eyebrow()),
    ];
    let listing_rows = inner.height.saturating_sub(10) as usize;
    for item in plan.items.iter().take(listing_rows.max(3)) {
        lines.push(Line::from(vec![
            Span::styled(
                widgets::pad_left(&widgets::bytes(item.bytes), 11),
                theme.text(),
            ),
            Span::styled(
                format!(
                    "  {}",
                    widgets::truncate_middle(
                        &widgets::short_path(&item.path),
                        width.saturating_sub(14)
                    )
                ),
                theme.muted(),
            ),
        ]));
    }
    if plan.items.len() > listing_rows.max(3) {
        lines.push(Line::styled(
            format!("… and {} more", plan.items.len() - listing_rows.max(3)),
            theme.faint(),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Authorization is separate. Type the phrase exactly, then press Enter:",
        theme.text(),
    ));
    lines.push(Line::styled(
        plan.approval_phrase.clone(),
        theme.bold(theme.accent),
    ));
    let matches = app.approval_matches();
    lines.push(Line::from(vec![
        Span::styled("▏", theme.faint()),
        Span::styled(app.approval.clone(), theme.text()),
        Span::styled("█", theme.color(widgets::pulse(&theme, app.tick))),
        Span::styled(
            if matches {
                "  ✓ matches · Enter authorizes quarantine"
            } else if app.approval.is_empty() {
                "  waiting for the exact phrase"
            } else {
                "  ✗ does not match yet"
            },
            if matches {
                theme.color(theme.teal)
            } else {
                theme.muted()
            },
        ),
    ]));
    lines.push(Line::raw(""));
    lines.push(widgets::hints(
        &theme,
        &[
            ("Enter", "authorize"),
            ("Esc", "discard plan"),
            ("^U", "clear input"),
        ],
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}
fn render_outcome(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let Some(operation) = &app.operation else {
        return;
    };
    let failed = operation.items.iter().any(|i| i.error.is_some());
    let block = card_accent(
        &theme,
        &format!("Operation {}", widgets::humanize(&operation.status)),
        if failed { theme.rose } else { theme.teal },
    );
    let width = block.inner(area).width as usize;
    let restorable = operation.items.iter().any(|i| i.status != "restored");
    let mut lines = vec![
        Line::from(vec![
            Span::styled(operation.id.clone(), theme.muted()),
            Span::styled(
                format!(
                    "  ·  {}  ·  {} files",
                    widgets::age(operation.created_at),
                    operation.items.len()
                ),
                theme.faint(),
            ),
        ]),
        Line::raw(""),
    ];
    for item in operation
        .items
        .iter()
        .take(area.height.saturating_sub(8) as usize)
    {
        let (glyph, color) = match item.status.as_str() {
            "restored" => ("↺", theme.teal),
            "moved" | "quarantined" => ("✓", theme.green),
            s if s.contains("fail") || item.error.is_some() => ("✗", theme.rose),
            _ => ("·", theme.muted),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{glyph} "), theme.bold(color)),
            Span::styled(
                format!("{:<11}", widgets::humanize(&item.status)),
                theme.text(),
            ),
            Span::styled(
                widgets::truncate_middle(
                    &widgets::short_path(&item.source),
                    width.saturating_sub(14),
                ),
                theme.muted(),
            ),
        ]));
        if let Some(error) = &item.error {
            for text in wrap_text(error, width.saturating_sub(4)) {
                lines.push(Line::styled(
                    format!("    {text}"),
                    theme.color(theme.amber),
                ));
            }
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        if restorable {
            "Quarantined bytes remain on the same filesystem and can be restored while their original location is available."
        } else {
            "Every file in this operation has been restored to its original location."
        },
        theme.faint(),
    ));
    let mut actions = vec![("Esc", "back to candidates"), ("o", "previous operations")];
    if restorable {
        actions.insert(0, ("u", "restore quarantined files"));
    }
    lines.push(widgets::hints(&theme, &actions));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}
