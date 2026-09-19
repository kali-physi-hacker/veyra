//! Applications: estimated footprints with evidence per associated location.
use super::{empty_state, wrap_text};
use crate::{
    app::App,
    widgets::{self, card_accent},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span, Text},
    widgets::{List, ListItem, Paragraph, Wrap},
};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    if app.apps.is_empty() {
        if !app.loading() {
            empty_state(
                frame,
                &theme,
                area,
                "No indexed application bundles",
                "Scan /Applications and relevant Library locations. Footprints only include paths actually observed.",
            );
        }
        return;
    }
    let wide = area.width >= 96;
    let (list_area, detail_area) = if wide {
        let [list, detail] =
            Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)])
                .spacing(1)
                .areas(area);
        (list, detail)
    } else {
        let [list, detail] =
            Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area);
        (list, detail)
    };
    let largest = app
        .apps
        .iter()
        .map(|a| a.footprint_bytes)
        .max()
        .unwrap_or(1)
        .max(1);
    let block = card_accent(&theme, "Applications", theme.rose);
    let width = block.inner(list_area).width.saturating_sub(2) as usize;
    let bar_width = if width >= 44 { 14 } else { 8 };
    let items: Vec<ListItem> = app
        .apps
        .iter()
        .take(100)
        .map(|application| {
            let size = widgets::bytes(application.footprint_bytes);
            let mut spans = vec![Span::styled(
                widgets::fit(
                    &application.name,
                    width.saturating_sub(bar_width + size.len() + 2),
                ),
                theme.strong(),
            )];
            spans.push(Span::raw(" "));
            spans.extend(widgets::bar(
                &theme,
                application.footprint_bytes as f64 / largest as f64,
                bar_width,
                theme.rose,
            ));
            spans.push(Span::styled(format!(" {size}"), theme.text()));
            ListItem::new(Text::from(vec![
                Line::from(spans),
                Line::styled(
                    widgets::truncate_end(
                        application
                            .bundle_id
                            .as_deref()
                            .unwrap_or("no bundle identifier"),
                        width,
                    ),
                    theme.muted(),
                ),
            ]))
        })
        .collect();
    let mut state = app.apps_nav.list_state();
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected())
            .highlight_symbol("▌ ")
            .block(block),
        list_area,
        &mut state,
    );
    app.apps_nav.offset = state.offset();
    let Some(application) = app
        .apps_nav
        .index(app.apps.len())
        .and_then(|i| app.apps.get(i))
    else {
        return;
    };
    let block = card_accent(&theme, "Estimated footprint", theme.accent);
    let width = block.inner(detail_area).width as usize;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                widgets::truncate_end(&application.name, width.saturating_sub(14)),
                theme.strong(),
            ),
            Span::raw(" "),
            widgets::badge(
                &theme,
                &widgets::bytes(application.footprint_bytes),
                theme.rose,
            ),
        ]),
        Line::styled(
            widgets::truncate_middle(&widgets::short_path(&application.path), width),
            theme.muted(),
        ),
    ];
    lines.extend(
        wrap_text(&application.coverage, width)
            .into_iter()
            .map(|l| Line::styled(l, theme.faint())),
    );
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!(
            "{} observed storage location{}",
            application.associations.len(),
            if application.associations.len() == 1 {
                ""
            } else {
                "s"
            }
        ),
        theme.eyebrow(),
    ));
    for association in &application.associations {
        lines.push(Line::from(vec![
            Span::styled("• ", theme.accent()),
            Span::styled(widgets::humanize(&association.kind), theme.strong()),
            Span::styled(
                format!("  {}  ", widgets::bytes(association.bytes)),
                theme.text(),
            ),
            widgets::badge(&theme, &association.confidence, theme.teal),
        ]));
        lines.push(Line::styled(
            format!(
                "  {}",
                widgets::truncate_middle(
                    &widgets::short_path(&association.path),
                    width.saturating_sub(2)
                )
            ),
            theme.muted(),
        ));
        for evidence in &association.evidence {
            for text in wrap_text(&evidence.detail, width.saturating_sub(4)) {
                lines.push(Line::styled(format!("    {text}"), theme.faint()));
            }
        }
    }
    if let Some((id, name, reason)) = &app.uninstall_review
        && name == &application.name
    {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Uninstall review · not executable",
            theme.eyebrow(),
        ));
        lines.extend(
            wrap_text(reason, width)
                .into_iter()
                .map(|l| Line::styled(l, theme.text())),
        );
        lines.push(Line::styled(format!("report {id}"), theme.faint()));
    }
    lines.push(Line::raw(""));
    lines.push(widgets::hints(
        &theme,
        &[("u", "record a non-executable uninstall review")],
    ));
    lines.push(Line::styled(
        "Bundle removal is not supported. Association evidence is not proof that shared data can be discarded.",
        theme.color(theme.amber),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block),
        detail_area,
    );
}
