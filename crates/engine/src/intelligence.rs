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

const CARGO_TARGET_REASON: &str = "File inside target beside a Cargo.toml manifest. Usually rebuildable, but local edits and active builds remain possible.";
const PACKAGE_CACHE_REASON: &str = "File inside a recognized downloaded package cache. Restoring dependencies may require network access.";

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
            ("developer_build_artifact", CARGO_TARGET_REASON, 0.95)
        } else if known_cache || cargo_registry {
            ("package_cache", PACKAGE_CACHE_REASON, 0.95)
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
        self.cached("insights", || {
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
        })
    }
    /// The folders whose files the cleanup rules recognise, largest first, found in the index
    /// rather than on one page of large files: Cargo target directories beside a Cargo.toml,
    /// npm's content cache and Cargo's registry cache. Directories carry their totals in the
    /// index, so this is one indexed lookup plus a manifest check per target directory.
    /// `scope` keeps only the folders inside one subtree.
    pub fn cleanup_locations(&self, scope: Option<&str>) -> Result<Vec<CleanupLocation>> {
        let find = || -> Result<Vec<CleanupLocation>> {
            let dirs = self.files(&FileQuery {
                kind: Some("directory".into()),
                names: vec!["target".into(), "_cacache".into(), "cache".into()],
                path: scope.map(str::to_string),
                limit: 1000,
                ..Default::default()
            })?;
            let mut found: Vec<CleanupLocation> = dirs
                .items
                .into_iter()
                .filter_map(|d| {
                    let path = Path::new(&d.path);
                    let parent = path.parent()?;
                    let (category, reason) = match d.name.as_str() {
                        "target" if parent.join("Cargo.toml").is_file() => ("developer_build_artifact", CARGO_TARGET_REASON),
                        "_cacache" if parent.file_name().is_some_and(|n| n == ".npm") => ("package_cache", PACKAGE_CACHE_REASON),
                        "cache" if d.path.ends_with("/.cargo/registry/cache") => ("package_cache", PACKAGE_CACHE_REASON),
                        _ => return None,
                    };
                    self.cleanup_path_allowed(path).ok()?;
                    Some(CleanupLocation {
                        path: d.path,
                        category: category.into(),
                        reason: reason.into(),
                        logical_bytes: d.logical_bytes,
                        allocated_bytes: d.allocated_bytes,
                        modified_at: d.modified_at,
                    })
                })
                .collect();
            // A folder inside another one is already counted in it.
            found.sort_by(|a, b| a.path.cmp(&b.path));
            let mut kept: Vec<CleanupLocation> = Vec::with_capacity(found.len());
            for loc in found {
                if kept.last().is_some_and(|k| loc.path.starts_with(&format!("{}/", k.path))) {
                    continue;
                }
                kept.push(loc);
            }
            kept.sort_by(|a, b| b.logical_bytes.cmp(&a.logical_bytes).then_with(|| a.path.cmp(&b.path)));
            Ok(kept)
        };
        match scope {
            None => self.cached("cleanup_locations", find),
            Some(_) => find(),
        }
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
