use crate::Engine;
use std::path::Path;
use stratum_domain::*;

pub struct AnalysisContext<'a> {
    pub engine: &'a Engine,
    pub observed_at: i64,
}
pub struct RuleMetadata {
    pub id: &'static str,
    pub description: &'static str,
    pub version: u32,
    pub risk: &'static str,
}
pub trait AnalysisRule: Send + Sync {
    fn metadata(&self) -> RuleMetadata;
    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Vec<Insight>>;
}
pub trait CleanupRule: Send + Sync {
    fn id(&self) -> &'static str;
    fn evaluate(&self, entry: &Entry) -> Option<CleanupCandidate>;
}

pub struct RegeneratableRule;
impl CleanupRule for RegeneratableRule {
    fn id(&self) -> &'static str {
        "known_regeneratable_v1"
    }
    fn evaluate(&self, e: &Entry) -> Option<CleanupCandidate> {
        if e.kind != EntryKind::File || e.identity.links != 1 {
            return None;
        }
        let path = Path::new(&e.path);
        let cargo_target = path.ancestors().find(|p| {
            p.file_name().is_some_and(|n| n == "target")
                && p.parent().is_some_and(|p| p.join("Cargo.toml").is_file())
        });
        let known_cache = path.ancestors().any(|p| {
            p.file_name().is_some_and(|n| n == ".npm")
                && path.components().any(|c| c.as_os_str() == "_cacache")
        });
        let cargo_registry = path.ancestors().any(|p| {
            p.file_name().is_some_and(|n| n == "cache")
                && p.parent()
                    .is_some_and(|p| p.file_name().is_some_and(|n| n == "registry"))
                && p.parent()
                    .and_then(Path::parent)
                    .is_some_and(|p| p.file_name().is_some_and(|n| n == ".cargo"))
        });
        let (category, reason, confidence) = if cargo_target.is_some() {
            (
                "developer_build_artifact",
                "File inside target beside a Cargo.toml manifest. Usually rebuildable, but local edits and active builds remain possible.",
                0.95,
            )
        } else if known_cache || cargo_registry {
            (
                "package_cache",
                "File inside a recognized downloaded package cache. Restoring dependencies may require network access.",
                0.95,
            )
        } else {
            return None;
        };
        Some(CleanupCandidate {
            id: blake3::hash(e.path.as_bytes()).to_hex().to_string(),
            path: e.path.clone(),
            size: e.logical_bytes,
            category: category.into(),
            reason: reason.into(),
            evidence: vec![Evidence::new(self.id(), reason)],
            confidence,
            risk: "moderate".into(),
            recommended_action: "quarantine".into(),
            reversible: true,
        })
    }
}
struct DeveloperRule;
impl AnalysisRule for DeveloperRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "developer_storage_v1",
            description: "Known development artifact directories",
            version: 1,
            risk: "moderate",
        }
    }
    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Vec<Insight>> {
        let mut out = vec![];
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
            for e in ctx
                .engine
                .files(&FileQuery {
                    kind: Some("directory".into()),
                    name: Some(name.into()),
                    limit: 100,
                    ..Default::default()
                })?
                .items
            {
                let confirmed =
                    name == "target" && Path::new(&e.parent).join("Cargo.toml").is_file();
                out.push(Insight{id:format!("dev-{}",blake3::hash(e.path.as_bytes()).to_hex()),kind:"developer_storage".into(),title:format!("{name} occupies {} bytes",e.logical_bytes),description:if confirmed{"Cargo build directory detected beside a Cargo.toml manifest. Rebuilding can recover generated files, but custom data may still be present."}else{"Directory name matches a development storage rule. Presence does not prove expendability."}.into(),evidence:vec![Evidence::new("indexed_directory",format!("{}: {} bytes",e.path,e.logical_bytes)),Evidence::new("rule",if confirmed{"Cargo manifest verified"}else{"Directory-name heuristic"})],confidence:if confirmed{0.95}else{0.75},severity:"info".into(),estimated_impact:e.logical_bytes,related_resources:vec![e.path],possible_actions:if confirmed{vec!["inspect_files".into(),"create_cleanup_plan".into()]}else{vec!["inspect_files".into()]},risk:"moderate".into(),created_at:ctx.observed_at});
            }
        }
        Ok(out)
    }
}
struct GrowthRule;
impl AnalysisRule for GrowthRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "directory_growth_v1",
            description: "Observed directory growth against historical baseline",
            version: 1,
            risk: "low",
        }
    }
    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Vec<Insight>> {
        let points = ctx.engine.history(None, ctx.observed_at - 7 * 86400)?;
        let mut paths = std::collections::BTreeMap::<String, Vec<HistoryPoint>>::new();
        for p in points.into_iter().filter(|p| p.coverage == "completed") {
            paths.entry(p.path.clone()).or_default().push(p);
        }
        let mut out = vec![];
        for (path, mut p) in paths {
            p.sort_by_key(|p| (p.timestamp, p.sequence));
            if p.len() < 2 {
                continue;
            }
            let first = &p[0];
            let last = &p[p.len() - 1];
            if last.logical_bytes <= first.logical_bytes {
                continue;
            }
            let delta = last.logical_bytes - first.logical_bytes;
            let ratio = delta as f64 / first.logical_bytes.max(1) as f64;
            if delta < 1024 * 1024 {
                continue;
            }
            let deltas: Vec<f64> = p
                .windows(2)
                .map(|v| v[1].logical_bytes as f64 - v[0].logical_bytes as f64)
                .collect();
            let anomalous = if deltas.len() >= 4 {
                let baseline = &deltas[..deltas.len() - 1];
                let avg = baseline.iter().sum::<f64>() / baseline.len() as f64;
                let sd = (baseline.iter().map(|v| (v - avg).powi(2)).sum::<f64>()
                    / baseline.len() as f64)
                    .sqrt();
                deltas[deltas.len() - 1] > avg + 3.0 * sd.max(1024.0 * 1024.0)
            } else {
                ratio > 0.5 && delta > 1024 * 1024 * 1024
            };
            out.push(Insight{id:format!("growth-{}",blake3::hash(path.as_bytes()).to_hex()),kind:if anomalous{"storage_growth_anomaly"}else{"storage_growth"}.into(),title:format!("Directory grew by {delta} bytes"),description:format!("Observed net growth between {} and {}. This window may be shorter than seven days; it does not identify every intermediate change.",first.timestamp,last.timestamp),evidence:vec![Evidence::new("historical_delta",format!("{} -> {} bytes; growth {:.1}%",first.logical_bytes,last.logical_bytes,ratio*100.0)),Evidence::new("anomaly_rule",if deltas.len()>=4{"Latest delta compared with previous mean plus three standard deviations (1 MiB minimum deviation)"}else{"Limited baseline: threshold is >50% and >1 GiB total growth"})],confidence:0.9,severity:if anomalous{"warning"}else{"info"}.into(),estimated_impact:delta,related_resources:vec![path],possible_actions:vec!["inspect_recent_files".into()],risk:"low".into(),created_at:ctx.observed_at});
        }
        Ok(out)
    }
}
impl Engine {
    pub fn insights(&self) -> Result<Vec<Insight>> {
        let ctx = AnalysisContext {
            engine: self,
            observed_at: now(),
        };
        let rules: Vec<Box<dyn AnalysisRule>> = vec![Box::new(DeveloperRule), Box::new(GrowthRule)];
        let mut insights = vec![];
        for rule in rules {
            insights.extend(rule.analyze(&ctx)?);
        }
        insights.sort_by_key(|i| std::cmp::Reverse(i.estimated_impact));
        Ok(insights)
    }
    pub fn cleanup_candidates(&self, query: &FileQuery) -> Result<Page<CleanupCandidate>> {
        let rule = RegeneratableRule;
        let q = FileQuery {
            kind: Some("file".into()),
            ..query.clone()
        };
        let entries = self.files(&q)?;
        let items = entries
            .items
            .iter()
            .filter_map(|e| rule.evaluate(e))
            .filter(|c| self.cleanup_path_allowed(Path::new(&c.path)).is_ok())
            .collect();
        Ok(Page {
            items,
            limit: entries.limit,
            offset: entries.offset,
            has_more: entries.has_more,
        })
    }
}
