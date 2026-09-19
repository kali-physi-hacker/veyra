//! Overview: capacity, indexed totals, locations and the findings worth attention.
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
    let Some(overview) = app.overview.clone() else {
        loading(
            frame,
            app,
            area,
            "Reading your local workspace · no scan or cleanup starts automatically",
        );
        return;
    };
    if overview.coverage.is_empty() {
        render_welcome(frame, &theme, area, app.tick);
        return;
    }
    let shown_scans = overview.coverage.len().min(4);
    let locations_lines = shown_scans * 2 + usize::from(overview.coverage.len() > shown_scans);
    let findings_lines = (overview.insights.len().min(6) * 2).max(4);
    let bottom_height = (locations_lines.max(findings_lines) as u16 + 2)
        .clamp(6, area.height.saturating_sub(9).max(6));
    let [top, bottom, _] = Layout::vertical([
        Constraint::Length(9),
        Constraint::Length(bottom_height),
        Constraint::Min(0),
    ])
    .areas(area);
    let [capacity, indexed] =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)])
            .spacing(1)
            .areas(top);
    render_capacity(frame, &theme, capacity, &overview.resources);
    render_indexed(frame, &theme, indexed, &overview);
    let [locations, findings] =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)])
            .spacing(1)
            .areas(bottom);
    render_locations(frame, &theme, locations, &overview.coverage);
    render_findings(frame, app, findings, &overview.insights);
}
fn render_welcome(frame: &mut Frame, theme: &Theme, area: Rect, tick: u64) {
    let rect = widgets::centered(area, area.width.min(68), area.height.min(16));
    let width = rect.width.saturating_sub(4) as usize;
    let mut lines = vec![
        Line::styled("A clearer picture starts with one folder", theme.eyebrow()),
        Line::raw(""),
        Line::styled("Meet your storage.", theme.strong()),
        Line::styled("Understand what matters.", theme.strong()),
        Line::raw(""),
    ];
    lines.extend(
        wrap_text(
            "Choose a folder to build a private, reusable index. Explore sizes, recognize development artifacts, and start a history of what changes.",
            width,
        )
        .into_iter()
        .map(|l| Line::styled(l, theme.muted())),
    );
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(" s ", theme.strong().bg(theme.raised)),
        Span::styled(" Choose a folder to scan", theme.bold(theme.teal)),
        Span::styled(
            format!("  {}", widgets::spinner(tick / 2)),
            theme.color(widgets::pulse(theme, tick)),
        ),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("01 ", theme.bold(theme.accent)),
        Span::styled("Observe   ", theme.text()),
        Span::styled("02 ", theme.bold(theme.accent)),
        Span::styled("Understand   ", theme.text()),
        Span::styled("03 ", theme.bold(theme.accent)),
        Span::styled("Decide", theme.text()),
    ]));
    lines.push(Line::styled(
        "Read-only discovery. No account. No cloud upload. Nothing is selected for cleanup.",
        theme.color(theme.teal),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(card(theme, "")),
        rect,
    );
}
fn render_capacity(frame: &mut Frame, theme: &Theme, area: Rect, resources: &ResourceSummary) {
    let block = card_accent(theme, "Capacity", theme.accent);
    let inner = block.inner(area);
    let width = inner.width as usize;
    let volume = resources
        .volumes
        .iter()
        .find(|v| v.mount == "/")
        .or_else(|| resources.volumes.first());
    let mut lines = Vec::new();
    if let Some(volume) = volume {
        let used = volume.total_bytes.saturating_sub(volume.available_bytes);
        let ratio = used as f64 / volume.total_bytes.max(1) as f64;
        lines.push(Line::from(vec![
            Span::styled(
                widgets::truncate_end(&volume.name, width.saturating_sub(16)),
                theme.strong(),
            ),
            Span::styled(
                format!("  {} · {}", volume.mount, volume.filesystem),
                theme.muted(),
            ),
        ]));
        let mut bar = widgets::bar(theme, ratio, width.saturating_sub(7), theme.accent);
        bar.push(Span::styled(
            format!(" {:>5}", widgets::percent(ratio)),
            theme.text(),
        ));
        lines.push(Line::from(bar));
        lines.push(Line::from(vec![
            Span::styled(
                widgets::bytes(volume.available_bytes),
                theme.bold(theme.teal),
            ),
            Span::styled(" available", theme.text()),
            Span::styled(
                format!(" of {}", widgets::bytes(volume.total_bytes)),
                theme.muted(),
            ),
        ]));
        lines.push(Line::styled(
            format!("{} used · OS measurement", widgets::bytes(used)),
            theme.muted(),
        ));
    } else {
        lines.push(Line::styled("No volume information", theme.muted()));
    }
    lines.push(Line::from(vec![
        Span::styled(format!("{:.0}% CPU", resources.cpu_percent), theme.text()),
        Span::styled(
            format!(
                " · {} / {} memory",
                widgets::compact_bytes(resources.used_memory),
                widgets::compact_bytes(resources.total_memory)
            ),
            theme.muted(),
        ),
    ]));
    lines.push(Line::styled(
        format!(
            "Sampled {} · not the indexed total",
            widgets::age(resources.timestamp)
        ),
        theme.faint(),
    ));
    let lines: Vec<Line> = lines
        .into_iter()
        .map(|line| clip_line(line, width))
        .collect();
    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}
/// Keep a styled line on one row by truncating its last span.
fn clip_line(mut line: Line<'static>, width: usize) -> Line<'static> {
    while line.width() > width {
        let excess = line.width() - width;
        let Some(last) = line.spans.last_mut() else {
            break;
        };
        let keep = widgets::width(&last.content).saturating_sub(excess);
        if keep == 0 {
            line.spans.pop();
            continue;
        }
        last.content = widgets::truncate_end(&last.content, keep).into();
    }
    line
}
fn render_indexed(frame: &mut Frame, theme: &Theme, area: Rect, overview: &StorageExplanation) {
    let block = card_accent(theme, "Indexed storage", theme.teal);
    let inner = block.inner(area);
    let width = inner.width as usize;
    let total: u64 = overview.categories.iter().map(|c| c.logical_bytes).sum();
    let files: u64 = overview.categories.iter().map(|c| c.files).sum();
    let warnings: u64 = overview.coverage.iter().map(|s| s.warnings).sum();
    let summary = format!(
        "{} files · {} · {} · logical",
        widgets::count(files),
        plural(overview.coverage.len(), "root", "roots"),
        if warnings == 0 {
            "no warnings".to_string()
        } else {
            format!("{warnings} warnings")
        }
    );
    let total_text = widgets::bytes(total);
    let mut lines = vec![Line::from(vec![
        Span::styled(total_text.clone(), theme.strong()),
        Span::styled(
            format!(
                "  {}",
                widgets::truncate_end(&summary, width.saturating_sub(total_text.len() + 2))
            ),
            theme.muted(),
        ),
    ])];
    let name_width = 20.min(width / 3);
    let bar_width = width.saturating_sub(name_width + 12);
    let colors = theme.categories();
    for (index, category) in overview
        .categories
        .iter()
        .take(inner.height.saturating_sub(1) as usize)
        .enumerate()
    {
        let ratio = category.logical_bytes as f64 / total.max(1) as f64;
        let mut spans = vec![Span::styled(
            widgets::fit(&widgets::humanize(&category.category), name_width),
            theme.text(),
        )];
        spans.push(Span::raw(" "));
        spans.extend(widgets::bar(
            theme,
            ratio,
            bar_width,
            colors[index % colors.len()],
        ));
        spans.push(Span::styled(
            widgets::pad_left(&widgets::bytes(category.logical_bytes), 11),
            theme.muted(),
        ));
        lines.push(Line::from(spans));
    }
    if overview.categories.is_empty() {
        lines.push(Line::styled(
            "No files have been categorized yet.",
            theme.muted(),
        ));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}
fn render_locations(frame: &mut Frame, theme: &Theme, area: Rect, coverage: &[ScanRecord]) {
    let block = card(theme, "Indexed locations");
    let inner = block.inner(area);
    let width = inner.width as usize;
    let mut lines = Vec::new();
    let shown = (inner.height as usize / 2).clamp(1, 4);
    for scan in coverage.iter().take(shown) {
        let color = widgets::freshness_color(theme, &scan.freshness);
        lines.push(Line::from(vec![
            Span::styled(widgets::status_glyph(&scan.status), theme.color(color)),
            Span::raw(" "),
            Span::styled(
                widgets::truncate_middle(
                    &widgets::short_path(&scan.root),
                    width.saturating_sub(12),
                ),
                theme.strong(),
            ),
            Span::raw(" "),
            widgets::badge(theme, widgets::freshness_label(&scan.freshness), color),
        ]));
        lines.push(Line::styled(
            widgets::truncate_end(
                &format!(
                    "  {} · {} entries · {} warnings · {} {}",
                    widgets::bytes(scan.logical_bytes),
                    widgets::count(scan.entries),
                    scan.warnings,
                    scan.status,
                    scan.completed_at
                        .map_or_else(|| "at an unknown time".into(), widgets::age)
                ),
                width,
            ),
            theme.muted(),
        ));
    }
    if coverage.len() > shown {
        lines.push(Line::styled(
            format!("+ {} more · press L on Storage", coverage.len() - shown),
            theme.faint(),
        ));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}
fn render_findings(frame: &mut Frame, app: &mut App, area: Rect, insights: &[Insight]) {
    let theme = app.theme;
    let block = card_accent(&theme, "Worth your attention", theme.amber);
    if insights.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        empty_state(
            frame,
            &theme,
            inner,
            "A baseline, not a verdict",
            "No rules produced findings in this scope. Rescan later to observe changes; the absence of findings is not a system health assessment.",
        );
        return;
    }
    let width = block.inner(area).width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = insights
        .iter()
        .take(6)
        .map(|insight| {
            let impact = widgets::bytes(insight.estimated_impact);
            let title_width = width.saturating_sub(impact.len() + 2);
            let path = insight
                .related_resources
                .first()
                .map(|p| widgets::short_path(p))
                .unwrap_or_default();
            ListItem::new(Text::from(vec![
                Line::from(vec![
                    Span::styled(widgets::fit(&insight.title, title_width), theme.strong()),
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
                    widgets::truncate_middle(
                        &format!(
                            "{} · {:.0}% confidence · {} risk · {}",
                            widgets::humanize(&insight.kind),
                            insight.confidence * 100.0,
                            insight.risk,
                            path
                        ),
                        width,
                    ),
                    theme.muted(),
                ),
            ]))
        })
        .collect();
    let mut state = app.findings_nav.list_state();
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected())
            .highlight_symbol("▌ ")
            .block(block),
        area,
        &mut state,
    );
    app.findings_nav.offset = state.offset();
}
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
