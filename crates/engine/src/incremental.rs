use crate::Engine;
use std::{collections::BTreeSet, path::Path};
use stratum_domain::*;
impl Engine {
    /// Native event adapters feed this method; overflow and directory events request full scans.
    pub fn reconcile_paths(&self, request: ReconcileRequest) -> Result<ReconcileReport> {
        let _guard = self.writer_lock()?;
        if request.paths.len() > 1024 {
            return Err(Error::invalid("At most 1024 changed paths per batch"));
        }
        let roots = self.store.roots()?;
        let mut fallback = BTreeSet::new();
        let mut changed = BTreeSet::new();
        let mut evidence = vec![];
        let mut updated = 0;
        for path in request.paths.into_iter().collect::<BTreeSet<_>>() {
            let p = Path::new(&path);
            if !p.is_absolute()
                || p.components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(Error::invalid(
                    "Changed paths must be absolute without traversal",
                ));
            }
            if p.starts_with(&self.config.data_dir) {
                continue;
            }
            let Some(root) = roots.iter().find(|root| p.starts_with(root)) else {
                return Err(Error::invalid("Changed path is outside indexed roots"));
            };
            self.store.mark_stale(root)?;
            let scan_request = self.scan_policy(root)?;
            // Custom policies are reapplied by a full reconciliation, never guessed here.
            if !scan_request.ignore_patterns.is_empty()
                || !scan_request.include_hidden
                || scan_request.exclusions.iter().any(|e| p.starts_with(e))
            {
                fallback.insert(root.clone());
                continue;
            }
            let depth = p
                .strip_prefix(root)
                .map_err(|_| Error::invalid("Invalid relative path"))?
                .components()
                .count() as u32;
            let replacement = match stratum_platform::scanner::read_entry(p, depth) {
                Ok(e) => {
                    if e.kind == EntryKind::File
                        && let Err(error) = stratum_platform::secure_fs::open_regular(p)
                    {
                        fallback.insert(root.clone());
                        evidence.push(Evidence::new(error.code, path));
                        continue;
                    }
                    let root_identity = self.store.entry(root)?.identity;
                    if !scan_request.cross_filesystems && e.identity.device != root_identity.device
                    {
                        fallback.insert(root.clone());
                        continue;
                    }
                    Some(e)
                }
                Err(e) if e.code == "path_not_found" => None,
                Err(e) => {
                    fallback.insert(root.clone());
                    evidence.push(Evidence::new(e.code, format!("{path}: {}", e.message)));
                    continue;
                }
            };
            if self.store.replace_leaf(root, &path, replacement.as_ref())? {
                updated += 1;
                changed.insert(root.clone());
            } else {
                fallback.insert(root.clone());
            }
        }
        self.store.snapshot_roots(
            &changed.into_iter().collect::<Vec<_>>(),
            self.config.history_retention_days,
        )?;
        self.store.audit(
            "incremental_update",
            "index",
            &format!(
                "{updated} paths; {} roots need reconciliation",
                fallback.len()
            ),
        )?;
        Ok(ReconcileReport {
            updated_paths: updated,
            roots_requiring_scan: fallback.into_iter().collect(),
            evidence,
        })
    }
}
