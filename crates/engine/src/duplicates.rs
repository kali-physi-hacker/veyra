use crate::Engine;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use stratum_domain::*;
use stratum_index::FingerprintUpdate;
use stratum_platform::secure_fs::hash_file;

impl Engine {
    pub fn discover_duplicates(&self, cancelled: &AtomicBool) -> Result<DuplicateReport> {
        if !cfg!(unix) {
            return Err(Error::new(
                "unsupported_platform_feature",
                "Duplicate verification requires stable Unix identities in release 0.1",
            ));
        }
        let _guard = self.writer_lock()?;
        let operation_id = id();
        self.emit(OperationEvent::DuplicateAnalysisStarted {
            operation_id: operation_id.clone(),
        });
        let mut report = DuplicateReport {
            analyzed_at: now(),
            group_count: 0,
            truncated: false,
            operation_id: operation_id.clone(),
            groups: vec![],
            warnings: vec![],
            files_hashed: 0,
            cancelled: false,
        };
        let mut after = String::new();
        loop {
            let entries = self.store.duplicate_candidates(&after)?;
            if entries.is_empty() {
                break;
            }
            let mut updates = vec![];
            for entry in entries {
                after = entry.path.clone();
                if cancelled.load(Ordering::Relaxed) {
                    report.cancelled = true;
                    break;
                }
                if !stratum_platform::scanner::read_entry(Path::new(&entry.path), entry.depth)
                    .is_ok_and(|e| e.identity == entry.identity)
                {
                    warning(&mut report, "filesystem_changed", entry.path);
                    continue;
                }
                if self
                    .store
                    .fingerprint(&entry.path, &entry.identity)?
                    .is_some()
                {
                    continue;
                }
                match hash_file(Path::new(&entry.path), true, || {
                    cancelled.load(Ordering::Relaxed)
                }) {
                    Ok((sample, identity)) if identity == entry.identity => {
                        updates.push(FingerprintUpdate {
                            path: entry.path,
                            identity,
                            sample,
                            full: None,
                        })
                    }
                    Ok(_) => warning(&mut report, "filesystem_changed", entry.path),
                    Err(e) => warning(
                        &mut report,
                        e.code,
                        format!("{}: {}", entry.path, e.message),
                    ),
                }
            }
            self.store.save_fingerprints(&updates)?;
            if report.cancelled {
                break;
            }
        }
        after.clear();
        while !report.cancelled {
            let entries = self.store.duplicate_candidates(&after)?;
            if entries.is_empty() {
                break;
            }
            let mut updates = vec![];
            for entry in entries {
                after = entry.path.clone();
                if cancelled.load(Ordering::Relaxed) {
                    report.cancelled = true;
                    break;
                }
                let Some((sample, cached)) =
                    self.store.fingerprint(&entry.path, &entry.identity)?
                else {
                    continue;
                };
                if self.store.sample_matches(&sample)? < 2 {
                    continue;
                }
                if !stratum_platform::scanner::read_entry(Path::new(&entry.path), entry.depth)
                    .is_ok_and(|e| e.kind == EntryKind::File && e.identity == entry.identity)
                {
                    warning(&mut report, "filesystem_changed", entry.path);
                    continue;
                }
                let full = if let Some(hash) = cached {
                    hash
                } else {
                    match hash_file(Path::new(&entry.path), false, || {
                        cancelled.load(Ordering::Relaxed)
                    }) {
                        Ok((hash, identity)) if identity == entry.identity => {
                            report.files_hashed += 1;
                            hash
                        }
                        Ok(_) => {
                            warning(&mut report, "filesystem_changed", entry.path);
                            continue;
                        }
                        Err(e) => {
                            warning(
                                &mut report,
                                e.code,
                                format!("{}: {}", entry.path, e.message),
                            );
                            continue;
                        }
                    }
                };
                updates.push(FingerprintUpdate {
                    path: entry.path,
                    identity: entry.identity,
                    sample,
                    full: Some(full),
                });
            }
            self.store.save_fingerprints(&updates)?;
            self.store.save_duplicate_members(&operation_id, &updates)?;
        }
        report.group_count = self.store.duplicate_group_count(&operation_id)?;
        let groups = self.store.duplicate_groups(&operation_id, 100, 0)?;
        report.truncated = groups.has_more
            || groups
                .items
                .iter()
                .any(|g| g.file_count > g.files.len() as u64);
        report.groups = groups.items;
        for group in &report.groups {
            self.emit(OperationEvent::DuplicateGroupFound {
                operation_id: operation_id.clone(),
                group_id: group.id.clone(),
            });
        }
        self.store.publish_duplicates(&report)?;
        self.emit(OperationEvent::DuplicateAnalysisCompleted {
            operation_id,
            groups: report.group_count,
        });
        Ok(report)
    }
    pub fn duplicates(&self) -> Result<DuplicateReport> {
        self.store.get("duplicates", "latest")
    }
    pub fn duplicate_groups(&self, limit: u32, offset: u64) -> Result<Page<DuplicateGroup>> {
        let report = self.duplicates()?;
        self.store
            .duplicate_groups(&report.operation_id, limit, offset)
    }
}
fn warning(report: &mut DuplicateReport, code: &str, detail: String) {
    if report.warnings.len() < 1000 {
        report.warnings.push(Evidence::new(code, detail));
    }
}
