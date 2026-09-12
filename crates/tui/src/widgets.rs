//! Pure rendering helpers: formatting, proportional bars, keycaps and cards.
use crate::theme::Theme;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Padding},
};
use stratum_engine::domain::EntryKind;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const EIGHTHS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

pub fn spinner(tick: u64) -> &'static str {
    SPINNER[(tick % SPINNER.len() as u64) as usize]
}
/// A color that breathes between the accent and the muted tone while work is active.
pub fn pulse(theme: &Theme, tick: u64) -> Color {
    match (tick / 3) % 4 {
        0 | 2 => theme.accent,
        1 => theme.lavender,
        _ => theme.muted,
    }
}
pub fn bytes(n: u64) -> String {
    let mut value = n as f64;
    let mut unit = "B";
    for u in ["KiB", "MiB", "GiB", "TiB"] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = u;
    }
    if unit == "B" {
        format!("{n} B")
    } else {
        format!("{value:.1} {unit}")
    }
}
/// Short form for tight columns, e.g. `12.7G`.
pub fn compact_bytes(n: u64) -> String {
    let mut value = n as f64;
    let mut unit = "";
    for u in ["K", "M", "G", "T"] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = u;
    }
    if unit.is_empty() {
        format!("{n}B")
    } else if value >= 100.0 {
        format!("{value:.0}{unit}")
    } else {
        format!("{value:.1}{unit}")
    }
}
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
pub fn percent(ratio: f64) -> String {
    if ratio.is_finite() {
        format!("{:.1}%", ratio.clamp(0.0, 1.0) * 100.0)
    } else {
        "—".into()
    }
}
pub fn age(timestamp: i64) -> String {
    let elapsed = stratum_engine::domain::now()
        .saturating_sub(timestamp)
        .max(0);
    if elapsed < 60 {
        "just now".into()
    } else if elapsed < 3600 {
        format!("{}m ago", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{}h ago", elapsed / 3600)
    } else {
        format!("{}d ago", elapsed / 86400)
    }
}
pub fn clock(seconds: u64) -> String {
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds % 3600 / 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}
pub fn short_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
        && let Ok(rest) = std::path::Path::new(path).strip_prefix(&home)
    {
        let rest = rest.display().to_string();
        return if rest.is_empty() {
            "~".into()
        } else {
            format!("~/{rest}")
        };
    }
    path.into()
}
pub fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}
/// Keep the beginning of a string, ending with an ellipsis when it does not fit.
pub fn truncate_end(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > max.saturating_sub(1) {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}
/// Keep the start and the end of a path-like string, eliding the middle.
pub fn truncate_middle(text: &str, max: usize) -> String {
    let total = width(text);
    if total <= max {
        return text.to_string();
    }
    if max < 4 {
        return truncate_end(text, max);
    }
    let keep_end = (max - 1) / 2;
    let keep_start = max - 1 - keep_end;
    let chars: Vec<char> = text.chars().collect();
    let mut start = String::new();
    let mut used = 0;
    for c in &chars {
        let w = UnicodeWidthChar::width(*c).unwrap_or(0);
        if used + w > keep_start {
            break;
        }
        start.push(*c);
        used += w;
    }
    let mut end = String::new();
    used = 0;
    for c in chars.iter().rev() {
        let w = UnicodeWidthChar::width(*c).unwrap_or(0);
        if used + w > keep_end {
            break;
        }
        end.insert(0, *c);
        used += w;
    }
    format!("{start}…{end}")
}
/// Truncate and pad to exactly `width` cells.
pub fn fit(text: &str, cells: usize) -> String {
    let mut out = truncate_end(text, cells);
    let w = width(&out);
    if w < cells {
        out.push_str(&" ".repeat(cells - w));
    }
    out
}
pub fn pad_left(text: &str, cells: usize) -> String {
    let w = width(text);
    if w >= cells {
        text.to_string()
    } else {
        format!("{}{text}", " ".repeat(cells - w))
    }
}
/// Filled cells for a ratio, using eighth-block glyphs for the fractional cell.
/// Returns the filled text and the number of unfilled cells.
pub fn bar_text(ratio: f64, cells: usize) -> (String, usize) {
    let ratio = if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = ratio * cells as f64;
    let mut full = filled.floor() as usize;
    let mut eighths = ((filled - full as f64) * 8.0).round() as usize;
    if eighths == 8 {
        full += 1;
        eighths = 0;
    }
    let full = full.min(cells);
    let mut text = "█".repeat(full);
    let mut used = full;
    if eighths > 0 && used < cells {
        text.push_str(EIGHTHS[eighths]);
        used += 1;
    }
    (text, cells - used)
}
pub fn bar<'a>(theme: &Theme, ratio: f64, cells: usize, color: Color) -> Vec<Span<'a>> {
    let (filled, rest) = bar_text(ratio, cells);
    vec![
        Span::styled(filled, Style::new().fg(color).bg(theme.raised)),
        Span::styled(" ".repeat(rest), Style::new().bg(theme.raised)),
    ]
}
/// Indeterminate progress: a bright segment sweeping across a dim track.
pub fn sweep<'a>(theme: &Theme, tick: u64, cells: usize, color: Color) -> Vec<Span<'a>> {
    if cells == 0 {
        return vec![];
    }
    let segment = (cells / 4).max(1);
    let period = cells + segment;
    let position = (tick * 2 % period as u64) as usize;
    let start = position.saturating_sub(segment);
    let end = position.min(cells);
    vec![
        Span::styled("━".repeat(start), Style::new().fg(theme.raised)),
        Span::styled(
            "━".repeat(end.saturating_sub(start)),
            Style::new().fg(color),
        ),
        Span::styled(
            "━".repeat(cells.saturating_sub(end)),
            Style::new().fg(theme.raised),
        ),
    ]
}
pub fn keycap<'a>(theme: &Theme, key: &str, label: &str) -> [Span<'a>; 2] {
    [
        Span::styled(
            format!(" {key} "),
            Style::new()
                .fg(theme.text)
                .bg(theme.raised)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(format!(" {label}  "), theme.muted()),
    ]
}
pub fn hints<'a>(theme: &Theme, items: &[(&str, &str)]) -> Line<'a> {
    let mut spans = vec![Span::raw(" ")];
    for (key, label) in items {
        spans.extend(keycap(theme, key, label));
    }
    Line::from(spans)
}
pub fn badge<'a>(theme: &Theme, text: &str, color: Color) -> Span<'a> {
    let _ = theme;
    Span::styled(
        format!(" {text} "),
        Style::new()
            .fg(color)
            .bg(Color::Reset)
            .add_modifier(ratatui::style::Modifier::REVERSED)
            .add_modifier(ratatui::style::Modifier::DIM),
    )
}
pub fn card<'a>(theme: &Theme, title: &str) -> Block<'a> {
    card_accent(theme, title, theme.text)
}
pub fn card_accent<'a>(theme: &Theme, title: &str, color: Color) -> Block<'a> {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.border())
        .padding(Padding::horizontal(1));
    if title.is_empty() {
        block
    } else {
        block.title(Line::from(vec![
            Span::raw(" "),
            Span::styled(title.to_string(), theme.bold(color)),
            Span::raw(" "),
        ]))
    }
}
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}
pub fn freshness_color(theme: &Theme, freshness: &str) -> Color {
    match freshness {
        "probably_fresh" => theme.teal,
        "stale" => theme.amber,
        _ => theme.muted,
    }
}
pub fn freshness_label(freshness: &str) -> &str {
    match freshness {
        "probably_fresh" => "fresh",
        other => other,
    }
}
pub fn kind_glyph(kind: &EntryKind) -> &'static str {
    match kind {
        EntryKind::Directory => "▸",
        EntryKind::File => "·",
        EntryKind::Symlink => "⇢",
        EntryKind::Other => "?",
    }
}
pub fn status_glyph(status: &str) -> &'static str {
    match status {
        "completed" | "moved" | "restored" | "probably_fresh" => "✓",
        "failed" | "error" => "✗",
        "partial" | "stale" | "skipped" => "⚠",
        "running" | "pending" => "…",
        _ => "·",
    }
}
pub fn humanize(text: &str) -> String {
    text.replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bytes_use_binary_units() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1024), "1.0 KiB");
        assert_eq!(bytes(13_631_488_000), "12.7 GiB");
        assert_eq!(compact_bytes(13_631_488_000), "12.7G");
        assert_eq!(compact_bytes(500), "500B");
        assert_eq!(compact_bytes(300 * 1024 * 1024), "300M");
    }
    #[test]
    fn counts_have_thousands_separators() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1000), "1,000");
        assert_eq!(count(1_234_567), "1,234,567");
    }
    #[test]
    fn bars_fill_proportionally_with_fractional_cells() {
        assert_eq!(bar_text(0.0, 10), (String::new(), 10));
        assert_eq!(bar_text(1.0, 10), ("█".repeat(10), 0));
        assert_eq!(bar_text(0.5, 10), ("█".repeat(5), 5));
        assert_eq!(bar_text(0.55, 10), (format!("{}▌", "█".repeat(5)), 4));
        assert_eq!(bar_text(f64::NAN, 10), (String::new(), 10));
        assert_eq!(bar_text(7.0, 4), ("████".into(), 0));
        let (text, rest) = bar_text(0.999, 8);
        assert_eq!(width(&text) + rest, 8);
    }
    #[test]
    fn truncation_respects_display_width() {
        assert_eq!(truncate_end("hello", 10), "hello");
        assert_eq!(truncate_end("hello world", 6), "hello…");
        assert_eq!(truncate_end("日本語テキスト", 5), "日本…");
        assert_eq!(
            truncate_middle("/Users/example/Projects/app", 12),
            "/Users…s/app"
        );
        assert_eq!(truncate_middle("short", 12), "short");
        assert_eq!(fit("ab", 5), "ab   ");
        assert_eq!(fit("abcdefgh", 5), "abcd…");
        assert_eq!(pad_left("42", 5), "   42");
    }
    #[test]
    fn sweep_covers_the_whole_track() {
        let theme = Theme::truecolor();
        for tick in 0..50 {
            let total: usize = sweep(&theme, tick, 20, theme.accent)
                .iter()
                .map(|s| width(&s.content))
                .sum();
            assert_eq!(total, 20, "tick {tick}");
        }
        assert!(sweep(&theme, 3, 0, theme.accent).is_empty());
    }
    #[test]
    fn clock_and_age_are_readable() {
        assert_eq!(clock(5), "00:05");
        assert_eq!(clock(125), "02:05");
        assert_eq!(clock(3725), "1:02:05");
        assert_eq!(age(stratum_engine::domain::now()), "just now");
        assert_eq!(age(stratum_engine::domain::now() - 7200), "2h ago");
    }
    #[test]
    fn centered_rects_stay_inside_the_area() {
        let area = Rect::new(0, 0, 100, 40);
        assert_eq!(centered(area, 60, 20), Rect::new(20, 10, 60, 20));
        assert_eq!(centered(area, 200, 200), area);
    }
}
