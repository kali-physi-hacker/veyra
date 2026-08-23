//! Formatting helpers shared by every page.
use stratum_domain::now;

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
    } else if value >= 100.0 {
        format!("{value:.0} {unit}")
    } else {
        format!("{value:.1} {unit}")
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

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    } else {
        s.into()
    }
}

/// Keep the end of a path visible, which is where the informative part usually is.
pub fn truncate_middle(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max || max < 5 {
        return s.into();
    }
    let head = max / 2 - 1;
    let tail = max - head - 1;
    let start: String = s.chars().take(head).collect();
    let end: String = s.chars().skip(count - tail).collect();
    format!("{start}…{end}")
}

pub fn age(timestamp: i64) -> String {
    let elapsed = now().saturating_sub(timestamp).max(0);
    if elapsed < 60 {
        "just now".into()
    } else if elapsed < 3600 {
        format!("{}m ago", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{}h ago", elapsed / 3600)
    } else if elapsed < 30 * 86400 {
        format!("{}d ago", elapsed / 86400)
    } else {
        format!("{}mo ago", elapsed / (30 * 86400))
    }
}

pub fn duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

pub fn short_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME")
        && let Ok(rest) = std::path::Path::new(path).strip_prefix(&home)
    {
        let rest = rest.display().to_string();
        return if rest.is_empty() {
            "~".into()
        } else {
            format!("~/{rest}").trim_end_matches('/').to_string()
        };
    }
    path.into()
}

pub fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

pub fn parent(path: &str) -> String {
    std::path::Path::new(path)
        .parent()
        .map_or_else(|| path.to_string(), |p| p.display().to_string())
}

pub fn humanize(word: &str) -> String {
    let mut out = word.replace('_', " ");
    if let Some(first) = out.get(..1) {
        let upper = first.to_uppercase();
        out.replace_range(..1, &upper);
    }
    out
}

/// Civil date (UTC) from a Unix timestamp, using Howard Hinnant's days-to-civil algorithm.
fn civil(timestamp: i64) -> (i64, u32, u32, u32, u32) {
    let days = timestamp.div_euclid(86400);
    let seconds = timestamp.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        m,
        d,
        (seconds / 3600) as u32,
        ((seconds % 3600) / 60) as u32,
    )
}

pub fn date(timestamp: i64) -> String {
    let (y, m, d, _, _) = civil(timestamp);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn datetime(timestamp: i64) -> String {
    let (y, m, d, hh, mm) = civil(timestamp);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02} UTC")
}

pub fn percent(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bytes_use_readable_precision() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(300 * 1024 * 1024), "300 MiB");
        assert_eq!(bytes(5 * 1024 * 1024 * 1024 + 512 * 1024 * 1024), "5.5 GiB");
    }
    #[test]
    fn counts_group_thousands() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1000), "1,000");
        assert_eq!(count(1234567), "1,234,567");
    }
    #[test]
    fn truncation_keeps_path_tails() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 6), "hello…");
        assert_eq!(
            truncate_middle("/very/long/path/to/file.txt", 15),
            "/very/…file.txt"
        );
        assert_eq!(truncate_middle("short", 15), "short");
    }
    #[test]
    fn humanize_capitalizes_snake_case() {
        assert_eq!(humanize("build_artifacts"), "Build artifacts");
        assert_eq!(humanize(""), "");
    }
    #[test]
    fn civil_dates_match_known_timestamps() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(datetime(951782400), "2000-02-29 00:00 UTC");
        assert_eq!(date(1_790_000_000), "2026-09-21");
        assert_eq!(datetime(-86400), "1969-12-31 00:00 UTC");
    }
    #[test]
    fn durations_and_percentages_are_bounded() {
        assert_eq!(duration(-5), "0s");
        assert_eq!(duration(65), "1m 5s");
        assert_eq!(duration(3700), "1h 1m");
        assert_eq!(percent(1, 0), 0.0);
        assert!((percent(1, 4) - 0.25).abs() < f32::EPSILON);
    }
}
