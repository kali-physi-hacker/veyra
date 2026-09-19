//! Storage browser: proportional rows for one directory plus an inspector.
use super::{empty_state, loading, wrap_text};
use crate::{
    app::App,
    theme::Theme,
    widgets::{self, card, card_accent},
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
    if app.path.is_empty() {
        empty_state(
            frame,
            &theme,
            area,
            "Your map starts with a scan",
            "Press s to choose a folder. Its files and folders will both appear here, in proportion.",
        );
        return;
    }
    let [crumb_row, body] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(4)]).areas(area);
    let totals = app.breakdown.as_ref().map(|b| {
        format!(
            "{} in {} direct children · logical",
            widgets::bytes(b.children_logical_bytes),
            widgets::count(b.child_count)
        )
    });
    let totals_width = totals
        .as_ref()
        .map_or(0, |t| t.len() as u16 + 2)
        .min(crumb_row.width / 2);
    let totals = totals.filter(|t| t.len() as u16 + 2 <= totals_width);
    frame.render_widget(
        breadcrumbs(
            &theme,
            &app.roots,
            &app.path,
            crumb_row.width.saturating_sub(totals_width) as usize,
        ),
        Rect::new(
            crumb_row.x,
            crumb_row.y,
            crumb_row.width.saturating_sub(totals_width),
            1,
        ),
    );
    if let Some(totals) = totals {
        frame.render_widget(
            Line::styled(totals, theme.muted()).right_aligned(),
            Rect::new(
                crumb_row.x + crumb_row.width.saturating_sub(totals_width),
                crumb_row.y,
                totals_width,
                1,
            ),
        );
    }
    let Some(breakdown) = app.breakdown.clone() else {
        loading(frame, app, body, "Reading directory totals");
        return;
    };
    if breakdown.child_count == 0 {
        empty_state(
            frame,
            &theme,
            body,
            "This indexed directory is empty",
            "No immediate children were recorded. Permission warnings and scan exclusions can also limit what was indexed.",
        );
        return;
    }
    let wide = body.width >= 96;
    let (list_area, inspector_area) = if wide {
        let [list, inspector] = Layout::horizontal([Constraint::Min(40), Constraint::Length(42)])
            .spacing(1)
            .areas(body);
        (list, inspector)
    } else {
        let [list, inspector] =
            Layout::vertical([Constraint::Min(4), Constraint::Length(9)]).areas(body);
        (list, inspector)
    };
    let total = breakdown.children_logical_bytes.max(1);
    let block = card_accent(&theme, "Contents", theme.teal);
    let width = block.inner(list_area).width.saturating_sub(2) as usize;
    let bar_width = if width >= 70 { 20 } else { 12 };
    let name_width = width.saturating_sub(bar_width + 22).max(10);
    let colors = theme.categories();
    let mut items: Vec<ListItem> = breakdown
        .children
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let ratio = entry.logical_bytes as f64 / total as f64;
            let color = colors[index % colors.len()];
            let name = if entry.kind == EntryKind::Directory {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            let mut spans = vec![
                Span::styled(widgets::kind_glyph(&entry.kind), theme.color(color)),
                Span::raw(" "),
                Span::styled(
                    widgets::fit(&name, name_width),
                    if entry.kind == EntryKind::Directory {
                        theme.text()
                    } else {
                        theme.muted()
                    },
                ),
                Span::raw(" "),
            ];
            spans.extend(widgets::bar(&theme, ratio, bar_width, color));
            spans.push(Span::styled(
                widgets::pad_left(&widgets::bytes(entry.logical_bytes), 11),
                theme.text(),
            ));
            spans.push(Span::styled(
                widgets::pad_left(&widgets::percent(ratio), 7),
                theme.faint(),
            ));
            ListItem::new(Line::from(spans))
        })
        .collect();
    if breakdown.omitted_count > 0 {
        let ratio = breakdown.omitted_logical_bytes as f64 / total as f64;
        let mut spans = vec![
            Span::styled("…", theme.faint()),
            Span::raw(" "),
            Span::styled(
                widgets::fit(
                    &format!("{} other items", widgets::count(breakdown.omitted_count)),
                    name_width,
                ),
                theme.muted(),
            ),
            Span::raw(" "),
        ];
        spans.extend(widgets::bar(&theme, ratio, bar_width, theme.faint));
        spans.push(Span::styled(
            widgets::pad_left(&widgets::bytes(breakdown.omitted_logical_bytes), 11),
            theme.muted(),
        ));
        spans.push(Span::styled(
            widgets::pad_left(&widgets::percent(ratio), 7),
            theme.faint(),
        ));
        items.push(ListItem::new(Line::from(spans)));
    }
    let mut state = app.storage_nav.list_state();
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected())
            .highlight_symbol("▌ ")
            .block(block),
        list_area,
        &mut state,
    );
    app.storage_nav.offset = state.offset();
    let rows = breakdown.children.len() + usize::from(breakdown.omitted_count > 0);
    match app.storage_nav.index(rows) {
        Some(index) if index < breakdown.children.len() => {
            render_inspector(frame, &theme, inspector_area, &breakdown.children[index]);
        }
        Some(_) => {
            frame.render_widget(
                Paragraph::new(Text::from(vec![
                    Line::styled(
                        format!(
                            "{} items beyond the map limit",
                            widgets::count(breakdown.omitted_count)
                        ),
                        theme.strong(),
                    ),
                    Line::styled(
                        format!(
                            "{} logical · {} allocated",
                            widgets::bytes(breakdown.omitted_logical_bytes),
                            widgets::bytes(breakdown.omitted_allocated_bytes)
                        ),
                        theme.muted(),
                    ),
                    Line::raw(""),
                    Line::styled(
                        "Press Enter to list every entry of this folder on the Files page.",
                        theme.muted(),
                    ),
                ]))
                .wrap(Wrap { trim: true })
                .block(card(&theme, "Remainder")),
                inspector_area,
            );
        }
        None => {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "Select an item to see details and classification evidence.",
                    theme.muted(),
                ))
                .wrap(Wrap { trim: true })
                .block(card(&theme, "Inspector")),
                inspector_area,
            );
        }
    }
}
fn breadcrumbs<'a>(theme: &Theme, roots: &[String], path: &str, max_width: usize) -> Line<'a> {
    let root = roots
        .iter()
        .filter(|r| path.starts_with(r.as_str()))
        .max_by_key(|r| r.len());
    let mut crumbs: Vec<String> = Vec::new();
    let rest = match root {
        Some(root) => {
            crumbs.push(widgets::short_path(root));
            path.strip_prefix(root.as_str()).unwrap_or("")
        }
        None => path,
    };
    crumbs.extend(
        rest.split('/')
            .filter(|c| !c.is_empty())
            .map(str::to_string),
    );
    let separator = " ▸ ";
    let mut total: usize = crumbs.iter().map(|c| widgets::width(c)).sum::<usize>()
        + separator.len() * crumbs.len().saturating_sub(1)
        + 2;
    let mut skipped = 0;
    while total > max_width && crumbs.len() > 1 {
        let removed = crumbs.remove(0);
        skipped += 1;
        total -= widgets::width(&removed) + separator.len();
    }
    let mut spans = vec![Span::styled("⌂ ", theme.color(theme.teal))];
    if skipped > 0 {
        spans.push(Span::styled("… ▸ ", theme.faint()));
    }
    let last = crumbs.len().saturating_sub(1);
    for (index, crumb) in crumbs.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(separator, theme.faint()));
        }
        spans.push(Span::styled(
            widgets::truncate_end(&crumb, max_width.max(8) - 4),
            if index == last {
                theme.strong()
            } else {
                theme.text()
            },
        ));
    }
    Line::from(spans)
}
fn render_inspector(frame: &mut Frame, theme: &Theme, area: Rect, entry: &Entry) {
    let block = card_accent(theme, "Inspector", theme.accent);
    let width = block.inner(area).width as usize;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                widgets::truncate_end(&entry.name, width.saturating_sub(12)),
                theme.strong(),
            ),
            Span::raw(" "),
            widgets::badge(theme, entry.kind.as_str(), theme.accent),
        ]),
        Line::styled(
            widgets::truncate_middle(&widgets::short_path(&entry.path), width),
            theme.muted(),
        ),
        Line::from(vec![
            Span::styled(widgets::bytes(entry.logical_bytes), theme.text()),
            Span::styled(" logical · ", theme.muted()),
            Span::styled(widgets::bytes(entry.allocated_bytes), theme.text()),
            Span::styled(" allocated", theme.muted()),
        ]),
        if entry.kind == EntryKind::Directory {
            Line::styled(
                "Directory totals sum every indexed descendant.",
                theme.muted(),
            )
        } else {
            Line::from(vec![
                Span::styled(widgets::humanize(&entry.category), theme.color(theme.teal)),
                Span::styled(
                    format!(
                        " · {:.0}% classification confidence",
                        entry.confidence * 100.0
                    ),
                    theme.muted(),
                ),
            ])
        },
        Line::styled(
            format!(
                "Modified {} · {} link{}",
                entry
                    .modified_at
                    .map_or_else(|| "unknown".into(), widgets::age),
                entry.identity.links,
                if entry.identity.links == 1 { "" } else { "s" }
            ),
            theme.muted(),
        ),
    ];
    if !entry.evidence.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("Evidence", theme.eyebrow()));
        for evidence in entry.evidence.iter().take(4) {
            for (index, text) in wrap_text(
                &format!("{}: {}", evidence.mechanism, evidence.detail),
                width.saturating_sub(2),
            )
            .into_iter()
            .enumerate()
            {
                lines.push(Line::from(vec![
                    Span::styled(if index == 0 { "• " } else { "  " }, theme.accent()),
                    Span::styled(text, theme.text()),
                ]));
            }
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Large or old does not mean expendable. Nothing here authorizes cleanup.",
        theme.faint(),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn breadcrumbs_start_at_the_indexed_root_and_elide_from_the_left() {
        let theme = Theme::truecolor();
        let roots = vec!["/tmp/root".to_string()];
        let line = breadcrumbs(&theme, &roots, "/tmp/root/Projects/atlas/target", 200);
        let text: String = line.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "⌂ /tmp/root ▸ Projects ▸ atlas ▸ target");
        let narrow = breadcrumbs(&theme, &roots, "/tmp/root/Projects/atlas/target", 24);
        let text: String = narrow.iter().map(|s| s.content.to_string()).collect();
        assert!(text.starts_with("⌂ … ▸ "), "{text}");
        assert!(text.ends_with("target"), "{text}");
        assert!(widgets::width(&text) <= 24, "{text}");
    }
}
