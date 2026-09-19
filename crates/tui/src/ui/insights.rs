//! Insights: deterministic findings with their evidence.
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
    if app.insights.is_empty() {
        if !app.loading() {
            empty_state(
                frame,
                &theme,
                area,
                "No findings in this scope",
                "Scan development folders or collect more observations over time. No findings is not a guarantee of system health.",
            );
        }
        return;
    }
    let wide = area.width >= 96;
    let (list_area, detail_area) = if wide {
        let [list, detail] =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                .spacing(1)
                .areas(area);
        (list, detail)
    } else {
        let [list, detail] =
            Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area);
        (list, detail)
    };
    let block = card_accent(&theme, "Findings", theme.amber);
    let width = block.inner(list_area).width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = app
        .insights
        .iter()
        .take(100)
        .map(|insight| {
            let impact = widgets::bytes(insight.estimated_impact);
            ListItem::new(Text::from(vec![
                Line::from(vec![
                    Span::styled(
                        widgets::fit(&insight.title, width.saturating_sub(impact.len() + 1)),
                        theme.strong(),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        impact,
                        theme.bold(if insight.severity == "warning" {
                            theme.amber
                        } else {
                            theme.accent
                        }),
                    ),
                ]),
                Line::styled(
                    widgets::truncate_end(
                        &format!(
                            "{} · {:.0}% confidence · {} risk",
                            widgets::humanize(&insight.kind),
                            insight.confidence * 100.0,
                            insight.risk
                        ),
                        width,
                    ),
                    theme.muted(),
                ),
            ]))
        })
        .collect();
    let mut state = app.insights_nav.list_state();
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected())
            .highlight_symbol("▌ ")
            .block(block),
        list_area,
        &mut state,
    );
    app.insights_nav.offset = state.offset();
    let Some(insight) = app
        .insights_nav
        .index(app.insights.len())
        .and_then(|i| app.insights.get(i))
    else {
        return;
    };
    let block = card_accent(&theme, "Why this finding?", theme.accent);
    let width = block.inner(detail_area).width as usize;
    let mut lines = vec![Line::from(vec![Span::styled(
        widgets::truncate_end(&insight.title, width),
        theme.strong(),
    )])];
    lines.push(Line::from(vec![
        widgets::badge(&theme, &widgets::humanize(&insight.kind), theme.amber),
        Span::raw(" "),
        widgets::badge(&theme, &format!("{} risk", insight.risk), theme.muted),
        Span::raw(" "),
        widgets::badge(
            &theme,
            &format!("{:.0}% confidence", insight.confidence * 100.0),
            theme.teal,
        ),
    ]));
    lines.push(Line::raw(""));
    lines.extend(
        wrap_text(&insight.description, width)
            .into_iter()
            .map(|l| Line::styled(l, theme.text())),
    );
    lines.push(Line::raw(""));
    let m = &insight.measurements;
    let mut measurements = vec![format!("{} logical", widgets::bytes(m.logical_bytes))];
    if let (Some(parent), Some(share)) = (m.parent_logical_bytes, m.share_of_parent_percent) {
        measurements.push(format!(
            "{share:.0}% of a {} parent",
            widgets::bytes(parent)
        ));
    }
    if let Some(growth) = m.growth_bytes_per_day {
        measurements.push(format!(
            "+{} per day",
            widgets::bytes(growth.max(0.0) as u64)
        ));
    }
    lines.push(Line::from(vec![
        Span::styled("Measured  ", theme.eyebrow()),
        Span::styled(measurements.join(" · "), theme.text()),
    ]));
    if let Some(path) = insight.related_resources.first() {
        lines.push(Line::from(vec![
            Span::styled("Location  ", theme.eyebrow()),
            Span::styled(
                widgets::truncate_middle(&widgets::short_path(path), width.saturating_sub(10)),
                theme.color(theme.teal),
            ),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("Evidence", theme.eyebrow()));
    for evidence in &insight.evidence {
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
    lines.push(Line::raw(""));
    let mut actions = vec![("Enter", "investigate in Storage")];
    if insight
        .possible_actions
        .iter()
        .any(|a| a == "create_cleanup_plan")
    {
        actions.push(("c", "review cleanup candidates"));
    }
    lines.push(widgets::hints(&theme, &actions));
    lines.push(Line::styled(
        "Impact values can overlap. Do not add them together as reclaimable space.",
        theme.color(theme.amber),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block),
        detail_area,
    );
}
