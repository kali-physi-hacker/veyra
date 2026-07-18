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
use crate::analysis_rules::{DeveloperRule, GrowthRule, RecentLargeRule};
impl Engine {
    pub fn insights(&self) -> Result<Vec<Insight>> {
        let ctx = AnalysisContext {
            engine: self,
            observed_at: now(),
        };
        let rules: Vec<Box<dyn AnalysisRule>> = vec![
            Box::new(DeveloperRule),
            Box::new(GrowthRule),
            Box::new(RecentLargeRule),
        ];
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
