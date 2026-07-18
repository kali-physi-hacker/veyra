use crate::intelligence::{AnalysisContext, AnalysisRule, RuleMetadata};
use std::{collections::BTreeMap, path::Path};
use stratum_domain::*;

pub struct DeveloperRule;
impl AnalysisRule for DeveloperRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "developer_storage_v2",
            description: "Development storage with parent context and nested suppression",
            version: 2,
            risk: "moderate",
        }
    }
    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Vec<Insight>> {
        let mut candidates = Vec::new();
        for name in [
            "target",
            "node_modules",
            "DerivedData",
            ".npm",
            ".gradle",
            ".m2",
            ".venv",
            ".git",
        ] {
            candidates.extend(
                ctx.engine
                    .files(&FileQuery {
                        kind: Some("directory".into()),
                        name: Some(name.into()),
                        limit: 100,
                        ..Default::default()
                    })?
                    .items,
            );
        }
        candidates.sort_by(|a, b| a.path.cmp(&b.path));
        let mut accepted: Vec<String> = Vec::new();
        let mut out = Vec::new();
        for entry in candidates {
            if entry.logical_bytes == 0
                || accepted
                    .iter()
                    .any(|p| Path::new(&entry.path).starts_with(p))
            {
                continue;
            }
            accepted.push(entry.path.clone());
            let confirmed =
                entry.name == "target" && Path::new(&entry.parent).join("Cargo.toml").is_file();
            let parent = ctx.engine.inspect_entry(&entry.parent).ok();
            let share = parent
                .as_ref()
                .filter(|p| p.logical_bytes > 0)
                .map(|p| entry.logical_bytes as f64 / p.logical_bytes as f64 * 100.0);
            let label = match entry.name.as_str() {
                "target" if confirmed => "Rust build artifacts",
                "node_modules" => "JavaScript dependencies",
                "DerivedData" => "Possible Xcode build data",
                ".git" => "Git history and repository data",
                ".venv" => "Python environment",
                ".npm" | ".gradle" | ".m2" => "Package and build tool data",
                _ => "Possible build output",
            };
            let title = share.map_or_else(
                || label.to_string(),
                |s| format!("{label} account for {s:.0}% of their parent"),
            );
            let description = if confirmed {
                "A target directory sits beside a Cargo.toml manifest. Build output is usually reproducible, but custom files and active builds need review."
            } else if entry.name == ".git" {
                "Repository history can contain unique, unpushed work. This is storage context, not a cleanup recommendation."
            } else {
                "A known directory name suggests development-related storage. Name evidence alone does not prove that every file is regeneratable."
            };
            let mut evidence = vec![
                Evidence::new("indexed_directory", &entry.path),
                Evidence::new(
                    "association",
                    if confirmed {
                        "Cargo manifest currently exists beside the directory"
                    } else {
                        "Directory-name heuristic; contents have not been proven expendable"
                    },
                ),
            ];
            if let Some(parent) = &parent {
                evidence.push(Evidence::new(
                    "parent_share",
                    format!(
                        "{} of {} indexed logical bytes in {}",
                        entry.logical_bytes, parent.logical_bytes, parent.path
                    ),
                ));
            }
            out.push(Insight {
                id: format!("dev-{}", blake3::hash(entry.path.as_bytes()).to_hex()),
                kind: "developer_storage".into(),
                title,
                description: description.into(),
                evidence,
                confidence: if confirmed { 0.95 } else { 0.75 },
                severity: "info".into(),
                estimated_impact: entry.logical_bytes,
                measurements: InsightMeasurements {
                    logical_bytes: entry.logical_bytes,
                    parent_logical_bytes: parent.as_ref().map(|p| p.logical_bytes),
                    share_of_parent_percent: share,
                    ..Default::default()
                },
                related_resources: vec![entry.path],
                possible_actions: if confirmed {
                    vec!["inspect_files".into(), "create_cleanup_plan".into()]
                } else {
                    vec!["inspect_files".into()]
                },
                risk: "moderate".into(),
                created_at: ctx.observed_at,
            });
        }
        Ok(out)
    }
}

pub struct GrowthRule;
impl AnalysisRule for GrowthRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "directory_growth_v2",
            description: "Time-normalized observed growth",
            version: 2,
            risk: "low",
        }
    }
    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Vec<Insight>> {
        Ok(growth_findings(
            ctx.engine.history(None, ctx.observed_at - 7 * 86400)?,
            ctx.observed_at,
        ))
    }
}
pub(crate) fn growth_findings(points: Vec<HistoryPoint>, observed_at: i64) -> Vec<Insight> {
    let mut paths = BTreeMap::<String, Vec<HistoryPoint>>::new();
    for point in points.into_iter().filter(|p| p.coverage == "completed") {
        paths.entry(point.path.clone()).or_default().push(point);
    }
    let mut out = Vec::new();
    for (path, mut points) in paths {
        points.sort_by_key(|p| (p.timestamp, p.sequence));
        let mut unique: Vec<HistoryPoint> = Vec::new();
        for point in points {
            if unique
                .last()
                .is_some_and(|p| p.timestamp == point.timestamp)
            {
                unique.pop();
            }
            unique.push(point);
        }
        if unique.len() < 2 {
            continue;
        }
        let first = &unique[0];
        let last = &unique[unique.len() - 1];
        let delta = last.logical_bytes.saturating_sub(first.logical_bytes);
        let seconds = last.timestamp - first.timestamp;
        if delta < 1024 * 1024 || seconds <= 0 {
            continue;
        }
        let rates: Vec<f64> = unique
            .windows(2)
            .map(|p| {
                (p[1].logical_bytes as f64 - p[0].logical_bytes as f64) * 86400.0
                    / (p[1].timestamp - p[0].timestamp) as f64
            })
            .collect();
        let ratio = delta as f64 / first.logical_bytes.max(1) as f64;
        let anomaly = if rates.len() >= 4 {
            let prior = &rates[..rates.len() - 1];
            let average = prior.iter().sum::<f64>() / prior.len() as f64;
            let deviation = (prior.iter().map(|r| (r - average).powi(2)).sum::<f64>()
                / prior.len() as f64)
                .sqrt();
            rates[rates.len() - 1] > average + 3.0 * deviation.max(1024.0 * 1024.0)
        } else {
            ratio > 0.5 && delta > 1024 * 1024 * 1024
        };
        let label = Path::new(&path)
            .file_name()
            .map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned());
        out.push(Insight {
            id: format!("growth-{}", blake3::hash(path.as_bytes()).to_hex()), kind: if anomaly {"storage_growth_anomaly"} else {"storage_growth"}.into(),
            title: format!("{label} grew {:.0}% during the observed window", ratio*100.0),
            description: format!("{} comparable observations across {:.1} days. This is measured net growth, not a prediction or a complete explanation of every change.", unique.len(), seconds as f64 / 86400.0),
            evidence: vec![Evidence::new("historical_delta", format!("{} → {} logical bytes between {} and {}", first.logical_bytes,last.logical_bytes,first.timestamp,last.timestamp)),Evidence::new("anomaly_rule",if rates.len()>=4 {"Rates normalized by elapsed time; latest rate compared with prior mean + 3 standard deviations (1 MiB/day minimum deviation)"} else {"Limited baseline; flag only >50% and >1 GiB observed growth"})],
            confidence: 0.9, severity: if anomaly {"warning"} else {"info"}.into(), estimated_impact: delta,
            measurements: InsightMeasurements { logical_bytes: last.logical_bytes, observation_start: Some(first.timestamp), observation_end: Some(last.timestamp), growth_bytes: Some(delta), growth_bytes_per_day: Some(delta as f64 * 86400.0 / seconds as f64), ..Default::default() },
            related_resources: vec![path], possible_actions: vec!["inspect_recent_files".into()], risk: "low".into(), created_at: observed_at,
        });
    }
    out
}

pub struct RecentLargeRule;
impl AnalysisRule for RecentLargeRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "recent_large_files_v1",
            description: "Large files with recent creation timestamps",
            version: 1,
            risk: "low",
        }
    }
    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Vec<Insight>> {
        Ok(ctx.engine.files(&FileQuery { kind: Some("file".into()), min_size: Some(100 * 1024 * 1024), created_after: Some(ctx.observed_at-7*86400), created_before: Some(ctx.observed_at+1), limit: 5, ..Default::default() })?.items.into_iter().map(|entry| Insight {
            id: format!("recent-{}", blake3::hash(entry.path.as_bytes()).to_hex()), kind: "recent_large_file".into(), title: format!("Recently created: {}", entry.name), description: "A file of at least 100 MiB has a creation timestamp within seven days. Copied or restored files may preserve timestamps; this is not evidence that the file is unnecessary.".into(), evidence: vec![Evidence::new("indexed_created_at", format!("{}; creation timestamp {}", entry.path,entry.created_at.unwrap_or_default()))], confidence: 0.9, severity: "info".into(), estimated_impact: entry.logical_bytes, measurements: InsightMeasurements { logical_bytes: entry.logical_bytes, observation_start: entry.created_at, observation_end: Some(ctx.observed_at), ..Default::default() }, related_resources: vec![entry.path], possible_actions: vec!["inspect_file".into()], risk: "low".into(), created_at: ctx.observed_at,
        }).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn point(day: i64, bytes: u64) -> HistoryPoint {
        HistoryPoint {
            sequence: day,
            scan_id: format!("scan-{day}"),
            path: "/data".into(),
            timestamp: day * 86400,
            logical_bytes: bytes,
            allocated_bytes: bytes,
            coverage: "completed".into(),
        }
    }
    #[test]
    fn irregular_intervals_with_steady_growth_are_not_anomalies() {
        let unit = 100 * 1024 * 1024;
        let findings = growth_findings(
            [0, 1, 2, 3, 6]
                .into_iter()
                .map(|day| point(day, (10 + day as u64) * unit))
                .collect(),
            7 * 86400,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, "storage_growth");
        assert_eq!(
            findings[0].measurements.growth_bytes_per_day,
            Some(unit as f64)
        );
    }
    #[test]
    fn growth_requires_distinct_timestamps_and_complete_coverage() {
        let mut second = point(1, 100 * 1024 * 1024);
        second.sequence = 99;
        assert!(growth_findings(vec![point(1, 0), second], 2 * 86400).is_empty());
        let mut partial = point(2, 100 * 1024 * 1024);
        partial.coverage = "partial".into();
        assert!(growth_findings(vec![point(1, 0), partial], 3 * 86400).is_empty());
    }
    #[test]
    fn rate_spike_has_explainable_observation_window() {
        let unit = 100 * 1024 * 1024;
        let mut points: Vec<_> = (0..4).map(|d| point(d, (10 + d as u64) * unit)).collect();
        points.push(point(4, 30 * unit));
        let finding = &growth_findings(points, 5 * 86400)[0];
        assert_eq!(finding.kind, "storage_growth_anomaly");
        assert_eq!(finding.measurements.observation_start, Some(0));
        assert_eq!(finding.measurements.observation_end, Some(4 * 86400));
    }
}
