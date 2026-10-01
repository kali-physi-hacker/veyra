//! Cleanup: choose exact files, review an immutable plan, authorize, inspect the outcome.
use super::{empty_state, wrap_text};
use crate::{
    app::{App, CleanupPhase, CleanupRow, PAGE_SIZE},
    widgets::{self, card_accent, count_label},
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
        let current =
            step == phase || (step == CleanupPhase::Outcome && phase == CleanupPhase::Purge);
        spans.push(Span::styled(
            label,
            if current {
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
                "Quarantine is reversible and frees no space on its own.",
                theme.bold(theme.amber),
            ),
            Span::styled(
                " A plan's DELETE phrase, or a later purge, frees it. Stop active builds first.",
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
        CleanupPhase::Purge => render_purge(frame, app, body),
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
    let ticked_bytes: u64 = app.selected_folders.values().sum();
    let total: u64 = app.locations.iter().map(|l| l.logical_bytes).sum();
    let mut summary = vec![
        Span::styled(count_label(app.locations.len(), true), theme.strong()),
        Span::styled(
            format!(" · {} in the index", widgets::bytes(total)),
            theme.muted(),
        ),
        Span::styled(
            format!("   {} ticked", app.selected_folders.len()),
            if app.selected_folders.is_empty() {
                theme.faint()
            } else {
                theme.bold(theme.teal)
            },
        ),
        Span::styled(
            format!(" · {}", widgets::bytes(ticked_bytes)),
            theme.faint(),
        ),
    ];
    if !app.selected.is_empty() {
        summary.push(Span::styled(
            format!(
                "   {} selected inside folders",
                count_label(app.selected.len(), false)
            ),
            theme.muted(),
        ));
    }
    frame.render_widget(Line::from(summary), bar);
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
        let path_width = width.saturating_sub(kind_width + cells + 18);
        let items: Vec<ListItem> = app
            .locations
            .iter()
            .map(|location| {
                let color = if location.category == "developer_build_artifact" {
                    theme.accent
                } else {
                    theme.teal
                };
                let ticked = app.selected_folders.contains_key(&location.path);
                let mut spans = vec![
                    Span::styled(
                        if ticked { "[x] " } else { "[ ] " },
                        if ticked {
                            theme.bold(theme.teal)
                        } else {
                            theme.faint()
                        },
                    ),
                    Span::styled(
                        widgets::fit(folder_kind(location), kind_width),
                        theme.color(color),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        widgets::fit(
                            &widgets::truncate_middle(
                                &widgets::short_path(&location.path),
                                path_width,
                            ),
                            path_width,
                        ),
                        theme.text(),
                    ),
                    Span::styled(
                        widgets::pad_left(&widgets::bytes(location.logical_bytes), 11),
                        theme.strong(),
                    ),
                    Span::raw(" "),
                ];
                spans.extend(widgets::bar(
                    &theme,
                    location.logical_bytes as f64 / largest as f64,
                    cells,
                    color,
                ));
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
            Span::styled("x tick · ⏎ open  ", theme.muted()),
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
/// The plan review. Either phrase runs it: `QUARANTINE` moves the items aside, `DELETE` also
/// purges them. The phrases and the input come before the list, so a short terminal cuts the
/// list instead.
fn render_plan(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let Some(plan) = &app.plan else {
        return;
    };
    let block = card_accent(&theme, "Immutable plan · nothing has moved", theme.amber);
    let inner = block.inner(area);
    let width = (inner.width as usize).max(1);
    let remaining = plan.expires_at.saturating_sub(now()).max(0);
    let folders = plan.items.iter().any(|i| i.folder.is_some());
    let files: u64 = plan
        .items
        .iter()
        .map(|i| i.folder.map_or(1, |f| f.files))
        .sum();
    let delete = delete_phrase(&plan.id);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(widgets::bytes(plan.total_bytes), theme.strong()),
            Span::styled(
                format!(
                    " in {}{} · {} risk · {}",
                    count_label(plan.items.len(), folders),
                    if folders {
                        format!(" · {}", count_label(files as usize, false))
                    } else {
                        String::new()
                    },
                    plan.risk,
                    if remaining == 0 {
                        "expired".to_string()
                    } else {
                        format!(
                            "expires in {} h {} min",
                            remaining / 3600,
                            remaining % 3600 / 60
                        )
                    }
                ),
                if remaining == 0 {
                    theme.color(theme.rose)
                } else {
                    theme.muted()
                },
            ),
        ]),
        Line::styled(
            "Authorization is separate. Type one phrase exactly, then press Enter:",
            theme.text(),
        ),
        Line::from(vec![
            Span::styled(plan.approval_phrase.clone(), theme.bold(theme.accent)),
            Span::styled("  quarantine · can be restored", theme.muted()),
        ]),
        Line::from(vec![
            Span::styled(delete.clone(), theme.bold(theme.rose)),
            Span::styled(
                if app.engine.config.purge_after_hours > 0 {
                    format!(
                        "  delete · only after {} h in quarantine",
                        app.engine.config.purge_after_hours
                    )
                } else {
                    "  delete permanently · cannot be undone".to_string()
                },
                theme.muted(),
            ),
        ]),
    ];
    let matches = app.approval_matches();
    let deleting = app.approval == delete;
    lines.push(Line::from(vec![
        Span::styled("▏", theme.faint()),
        Span::styled(app.approval.clone(), theme.text()),
        Span::styled("█", theme.color(widgets::pulse(&theme, app.tick))),
        Span::styled(
            if matches && deleting {
                "  ✓ matches · Enter deletes permanently"
            } else if matches {
                "  ✓ matches · Enter authorizes quarantine"
            } else if app.approval.is_empty() {
                "  waiting for an exact phrase"
            } else {
                "  ✗ does not match yet"
            },
            if matches && deleting {
                theme.color(theme.rose)
            } else if matches {
                theme.color(theme.teal)
            } else {
                theme.muted()
            },
        ),
    ]));
    lines.push(widgets::hints(
        &theme,
        &[
            ("Enter", "run"),
            ("Esc", "discard plan"),
            ("^U", "clear input"),
        ],
    ));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        if folders {
            "Exact folders in this plan"
        } else {
            "Exact files in this plan"
        },
        theme.eyebrow(),
    ));
    // Whatever height the wrapped lines above leave, less one line for "… and N more".
    let used: usize = lines.iter().map(|l| l.width().max(1).div_ceil(width)).sum();
    let room = (inner.height as usize).saturating_sub(used + 1);
    let shown = plan.items.len().min(room);
    for item in plan.items.iter().take(shown) {
        let path = widgets::short_path(&item.path);
        let label = match item.folder {
            Some(folder) => format!("{path} · {}", count_label(folder.files as usize, false)),
            None => path,
        };
        lines.push(Line::from(vec![
            Span::styled(
                widgets::pad_left(&widgets::bytes(item.bytes), 11),
                theme.text(),
            ),
            Span::styled(
                format!(
                    "  {}",
                    widgets::truncate_middle(&label, width.saturating_sub(14))
                ),
                theme.muted(),
            ),
        ]));
    }
    if plan.items.len() > shown {
        lines.push(Line::styled(
            format!("… and {} more", plan.items.len() - shown),
            theme.faint(),
        ));
    }
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
    let restorable = operation.restorable();
    let held = operation.purgeable().count();
    let mut lines = vec![
        Line::from(vec![
            Span::styled(operation.id.clone(), theme.muted()),
            Span::styled(
                format!(
                    "  ·  {}  ·  {}",
                    widgets::age(operation.created_at),
                    count_label(
                        operation.items.len(),
                        operation.items.iter().any(|i| i.folder.is_some())
                    )
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
            "purged" => ("∅", theme.muted),
            "missing" => ("?", theme.amber),
            "purging" | "moving" => ("…", theme.amber),
            s if s.contains("fail") || item.error.is_some() => ("✗", theme.rose),
            _ => ("·", theme.muted),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{glyph} "), theme.bold(color)),
            // Wide enough for "restore failed", with a space before the path.
            Span::styled(
                format!("{:<15}", widgets::humanize(&item.status)),
                theme.text(),
            ),
            Span::styled(
                widgets::truncate_middle(
                    &match item.folder {
                        Some(folder) => format!(
                            "{} · folder of {}",
                            widgets::short_path(&item.source),
                            count_label(folder.files as usize, false)
                        ),
                        None => widgets::short_path(&item.source),
                    },
                    width.saturating_sub(18),
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
    let purged = operation.purged_bytes();
    lines.push(Line::styled(
        if held > 0 {
            format!(
                "{} · {} held in quarantine on the same filesystem. They can be restored while their original location is free, or deleted permanently.",
                count_label(held, operation.items.iter().any(|i| i.folder.is_some())),
                widgets::bytes(operation.purgeable_bytes())
            )
        } else if purged > 0 {
            format!(
                "{} deleted permanently; nothing from this operation is left in quarantine.",
                widgets::bytes(purged)
            )
        } else if restorable {
            "Some files need review before they can be restored.".to_string()
        } else {
            "Every file in this operation has been restored to its original location.".to_string()
        },
        theme.faint(),
    ));
    let mut actions = vec![("Esc", "back"), ("o", "operations")];
    if held > 0 {
        actions.insert(0, ("D", "delete permanently"));
    }
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
/// The separate, typed authorization for deleting what an operation holds in quarantine. The
/// phrase and its input come before the file list, so a short terminal cuts the list instead.
fn render_purge(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let Some(operation) = &app.operation else {
        return;
    };
    let held: Vec<&QuarantineItem> = operation.purgeable().collect();
    let folders = held.iter().any(|i| i.folder.is_some());
    let block = card_accent(
        &theme,
        "Delete permanently · this cannot be undone",
        theme.rose,
    );
    let inner = block.inner(area);
    let width = (inner.width as usize).max(1);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(widgets::bytes(operation.purgeable_bytes()), theme.strong()),
            Span::styled(
                format!(" in {} in quarantine", count_label(held.len(), folders)),
                theme.muted(),
            ),
        ]),
        Line::styled(
            if folders {
                "Everything inside skips the Trash and Stratum cannot restore it. A folder goes only while it is still the one that moved; symlinks inside are removed, never followed."
            } else {
                "The files skip the Trash and Stratum cannot restore them. Each is checked against the content hash recorded when it moved; anything that changed stays in quarantine."
            },
            theme.text(),
        ),
        Line::raw(""),
    ];
    let wait = app.purge_wait();
    if wait > 0 {
        lines.push(Line::styled(
            format!(
                "Files stay in quarantine for {} h before they can be deleted; this operation is ready in {} min.",
                app.engine.config.purge_after_hours,
                (wait + 59) / 60
            ),
            theme.color(theme.amber),
        ));
    } else {
        lines.push(Line::styled(
            "Authorization is separate from the quarantine. Type the phrase exactly, then press Enter:",
            theme.text(),
        ));
        lines.push(Line::styled(
            purge_phrase(&operation.id),
            theme.bold(theme.rose),
        ));
        let matches = app.purge_matches();
        lines.push(Line::from(vec![
            Span::styled("▏", theme.faint()),
            Span::styled(app.purge_typed.clone(), theme.text()),
            Span::styled("█", theme.color(widgets::pulse(&theme, app.tick))),
            Span::styled(
                if matches {
                    "  ✓ matches · Enter deletes permanently"
                } else if app.purge_typed.is_empty() {
                    "  waiting for the exact phrase"
                } else {
                    "  ✗ does not match yet"
                },
                if matches {
                    theme.color(theme.rose)
                } else {
                    theme.muted()
                },
            ),
        ]));
    }
    lines.push(widgets::hints(
        &theme,
        &[
            ("Enter", "delete permanently"),
            ("Esc", "keep in quarantine"),
            ("^U", "clear input"),
        ],
    ));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        if folders {
            "Folders to delete"
        } else {
            "Files to delete"
        },
        theme.eyebrow(),
    ));
    // Whatever height the wrapped lines above leave, less one line for "… and N more".
    let used: usize = lines.iter().map(|l| l.width().max(1).div_ceil(width)).sum();
    let room = (inner.height as usize).saturating_sub(used + 1);
    let shown = if held.len() > room { room } else { held.len() };
    for item in held.iter().take(shown) {
        lines.push(Line::from(vec![
            Span::styled(
                widgets::pad_left(&widgets::bytes(item.bytes()), 11),
                theme.text(),
            ),
            Span::styled(
                format!(
                    "  {}",
                    widgets::truncate_middle(
                        &widgets::short_path(&item.source),
                        width.saturating_sub(14)
                    )
                ),
                theme.muted(),
            ),
        ]));
    }
    if held.len() > shown {
        lines.push(Line::styled(
            format!("… and {} more", held.len() - shown),
            theme.faint(),
        ));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}
