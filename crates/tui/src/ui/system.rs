//! System: gauges, a live sparkline and the process table.
use super::loading;
use crate::{
    app::{App, ProcessSort},
    theme::Theme,
    widgets::{self, card, card_accent},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Cell, Gauge, Paragraph, Row, Sparkline, Table},
};
use stratum_engine::domain::*;

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let Some(snapshot) = app.system.clone() else {
        loading(frame, app, area, "Sampling CPU, memory and processes");
        return;
    };
    let [gauges, charts, processes] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(7),
        Constraint::Min(4),
    ])
    .areas(area);
    let [cpu, memory, swap] = Layout::horizontal([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .spacing(1)
    .areas(gauges);
    let load = snapshot
        .load_average
        .iter()
        .map(|l| format!("{l:.2}"))
        .collect::<Vec<_>>()
        .join(" ");
    gauge(
        frame,
        &theme,
        cpu,
        "CPU",
        f64::from(snapshot.cpu_percent) / 100.0,
        &format!("{:.1}%", snapshot.cpu_percent),
        &format!("load {load} · sampled over {} ms", snapshot.sample_millis),
        theme.cyan,
    );
    gauge(
        frame,
        &theme,
        memory,
        "Memory",
        snapshot.used_memory as f64 / snapshot.total_memory.max(1) as f64,
        &widgets::bytes(snapshot.used_memory),
        &format!(
            "of {}{}",
            widgets::bytes(snapshot.total_memory),
            snapshot
                .memory_pressure
                .as_deref()
                .map_or(String::new(), |p| format!(" · pressure {p}"))
        ),
        theme.accent,
    );
    gauge(
        frame,
        &theme,
        swap,
        "Swap",
        snapshot.used_swap as f64 / snapshot.total_swap.max(1) as f64,
        &widgets::bytes(snapshot.used_swap),
        &format!("of {}", widgets::bytes(snapshot.total_swap)),
        theme.amber,
    );
    let [history, volumes] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
            .spacing(1)
            .areas(charts);
    render_history(frame, app, history);
    render_volumes(frame, &theme, volumes, &snapshot);
    render_processes(frame, app, processes, &snapshot);
}
#[allow(clippy::too_many_arguments)]
fn gauge(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    title: &str,
    ratio: f64,
    value: &str,
    detail: &str,
    color: Color,
) {
    let block = card_accent(theme, title, color);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return;
    }
    let ratio = if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.0
    };
    frame.render_widget(
        Gauge::default()
            .ratio(ratio)
            .label(Span::styled(value.to_string(), theme.strong()))
            .gauge_style(Style::new().fg(color).bg(theme.raised))
            .use_unicode(true),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    if inner.height > 1 {
        frame.render_widget(
            Line::styled(
                widgets::truncate_end(detail, inner.width as usize),
                theme.muted(),
            ),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }
}
fn render_history(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let block = card(
        &theme,
        &format!(
            "CPU while this page is open · {} sample{}",
            app.cpu_history.len(),
            if app.cpu_history.len() == 1 { "" } else { "s" }
        ),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 2 {
        return;
    }
    let width = inner.width as usize;
    let cpu: Vec<u64> = app
        .cpu_history
        .iter()
        .rev()
        .take(width)
        .rev()
        .copied()
        .collect();
    frame.render_widget(
        Sparkline::default()
            .data(cpu)
            .max(100)
            .style(theme.color(theme.cyan)),
        Rect::new(inner.x, inner.y, inner.width, inner.height - 1),
    );
    let memory: Vec<u64> = app
        .memory_history
        .iter()
        .rev()
        .take(width)
        .rev()
        .copied()
        .collect();
    let latest_memory = memory.last().copied().unwrap_or(0);
    frame.render_widget(
        Line::from(vec![
            Span::styled("memory ", theme.faint()),
            Span::styled(format!("{latest_memory}% "), theme.color(theme.accent)),
            Span::styled(
                memory.iter().map(|v| tiny_bar(*v)).collect::<String>(),
                theme.color(theme.accent),
            ),
        ]),
        Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
    );
}
fn tiny_bar(percent: u64) -> char {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    LEVELS[((percent.min(100) * 7 + 50) / 100) as usize]
}
fn render_volumes(frame: &mut Frame, theme: &Theme, area: Rect, snapshot: &SystemSnapshot) {
    let block = card(theme, "Volumes");
    let width = block.inner(area).width as usize;
    let mut lines = Vec::new();
    for volume in snapshot
        .volumes
        .iter()
        .take(block.inner(area).height as usize)
    {
        let used = volume.total_bytes.saturating_sub(volume.available_bytes);
        let ratio = used as f64 / volume.total_bytes.max(1) as f64;
        let label = widgets::fit(&widgets::truncate_end(&volume.mount, 16), 16);
        let mut spans = vec![
            Span::styled(
                label,
                if volume.removable {
                    theme.color(theme.cyan)
                } else {
                    theme.text()
                },
            ),
            Span::raw(" "),
        ];
        spans.extend(widgets::bar(
            theme,
            ratio,
            width.saturating_sub(40).max(6),
            theme.accent,
        ));
        spans.push(Span::styled(
            format!(
                " {} free of {}",
                widgets::compact_bytes(volume.available_bytes),
                widgets::compact_bytes(volume.total_bytes)
            ),
            theme.muted(),
        ));
        lines.push(Line::from(spans));
    }
    if snapshot.volumes.is_empty() {
        lines.push(Line::styled("No volumes reported", theme.muted()));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}
fn render_processes(frame: &mut Frame, app: &mut App, area: Rect, snapshot: &SystemSnapshot) {
    let theme = app.theme;
    let mut processes: Vec<&ProcessSnapshot> = snapshot.processes.iter().collect();
    match app.process_sort {
        ProcessSort::Memory => processes.sort_by_key(|p| std::cmp::Reverse(p.memory_bytes)),
        ProcessSort::Cpu => processes.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    }
    processes.truncate(100);
    let sort_label = match app.process_sort {
        ProcessSort::Memory => "highest memory first",
        ProcessSort::Cpu => "highest CPU first",
    };
    let header = Row::new(
        ["PID", "PROCESS", "CPU", "MEMORY", "WRITTEN", "RUNTIME"]
            .into_iter()
            .map(|h| Cell::from(Span::styled(h, theme.eyebrow()))),
    );
    let rows = processes.iter().map(|p| {
        Row::new(vec![
            Cell::from(Span::styled(p.pid.to_string(), theme.muted())),
            Cell::from(Span::styled(p.name.clone(), theme.text())),
            Cell::from(Span::styled(
                format!("{:.1}%", p.cpu_percent),
                theme.color(if p.cpu_percent >= 50.0 {
                    theme.amber
                } else {
                    theme.text
                }),
            )),
            Cell::from(Span::styled(widgets::bytes(p.memory_bytes), theme.text())),
            Cell::from(Span::styled(
                widgets::bytes(p.disk_written_bytes),
                theme.muted(),
            )),
            Cell::from(Span::styled(
                widgets::clock(p.runtime_seconds),
                theme.muted(),
            )),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Min(16),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(9),
        ],
    )
    .header(header)
    .column_spacing(2)
    .row_highlight_style(theme.selected())
    .highlight_symbol("▌ ")
    .block(card(
        &theme,
        &format!("Processes · {sort_label} · o toggles"),
    ));
    let mut state = app.process_nav.table_state();
    frame.render_stateful_widget(table, area, &mut state);
    app.process_nav.offset = state.offset();
    if !snapshot.limitations.is_empty() && area.height > 6 {
        let text = widgets::truncate_end(
            &snapshot.limitations.join(" · "),
            area.width.saturating_sub(6) as usize,
        );
        frame.render_widget(
            Line::styled(format!(" {text} "), theme.faint().bg(theme.bg)).right_aligned(),
            Rect::new(
                area.x + 2,
                area.bottom() - 1,
                area.width.saturating_sub(4),
                1,
            ),
        );
    }
}
