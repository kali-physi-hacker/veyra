use std::{
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
use stratum_engine::{Engine, domain::*};
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    engine: Arc<Engine>,
}
impl Fixture {
    fn store(&self) -> stratum_index::Store {
        stratum_index::Store::open(&self.engine.config.data_dir.join("index.sqlite3")).unwrap()
    }

    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(temp.path()).unwrap();
        let root = base.join("data");
        fs::create_dir_all(root.join("project/target/debug")).unwrap();
        fs::write(
            root.join("project/Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(
            root.join("project/target/debug/artifact"),
            "build artifact contents",
        )
        .unwrap();
        let engine = Engine::open(Config {
            data_dir: base.join("state"),
            batch_size: 2,
            scan_queue_capacity: 2,
            ..Default::default()
        })
        .unwrap();
        Self {
            _temp: temp,
            root,
            engine,
        }
    }
    fn scan(&self) -> Vec<ScanRecord> {
        self.engine
            .scan(ScanRequest {
                roots: vec![self.root.to_string_lossy().into()],
                ..Default::default()
            })
            .unwrap()
    }
    fn artifact(&self) -> String {
        self.root
            .join("project/target/debug/artifact")
            .to_string_lossy()
            .into()
    }
    fn plan(&self) -> CleanupPlan {
        self.engine
            .create_cleanup_plan(PlanRequest {
                paths: vec![self.artifact()],
            })
            .unwrap()
    }
}
#[test]
fn persistence_query_reconciliation_and_history() {
    let f = Fixture::new();
    f.scan();
    assert!(
        f.engine
            .insights()
            .unwrap()
            .iter()
            .any(|i| i.kind == "developer_storage")
    );
    let initial = f
        .engine
        .files(&FileQuery {
            kind: Some("file".into()),
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(initial.items.len(), 1);
    assert!(initial.has_more);
    fs::write(f.root.join("new.bin"), vec![42; 2 * 1024 * 1024]).unwrap();
    fs::remove_file(f.artifact()).unwrap();
    f.scan();
    let e = f.store().entry(f.root.to_str().unwrap()).unwrap();
    assert!(e.logical_bytes >= 2 * 1024 * 1024);
    assert!(f.store().entry(&f.artifact()).is_err());
    let reopened = Engine::open(f.engine.config.clone()).unwrap();
    assert_eq!(
        reopened
            .categories()
            .unwrap()
            .iter()
            .map(|c| c.logical_bytes)
            .sum::<u64>(),
        e.logical_bytes
    );
    assert!(
        reopened
            .history(Some(f.root.to_str().unwrap()), 0)
            .unwrap()
            .len()
            >= 2
    );
    assert!(
        reopened
            .insights()
            .unwrap()
            .iter()
            .all(|i| i.estimated_impact > 0)
    );
}

#[test]
fn breakdown_includes_direct_files_and_accounts_for_remainder() {
    let f = Fixture::new();
    fs::write(f.root.join("direct.bin"), vec![1; 4096]).unwrap();
    fs::write(f.root.join("empty"), []).unwrap();
    f.scan();
    let map = f
        .engine
        .directory_breakdown(f.root.to_str().unwrap(), 1)
        .unwrap();
    assert_eq!(map.children[0].name, "direct.bin");
    assert_eq!(map.children[0].kind, EntryKind::File);
    assert_eq!(map.child_count, 3);
    assert_eq!(map.omitted_count, 2);
    assert_eq!(
        map.children_logical_bytes,
        map.children.iter().map(|e| e.logical_bytes).sum::<u64>() + map.omitted_logical_bytes
    );
    assert_eq!(
        map.children_allocated_bytes,
        map.children.iter().map(|e| e.allocated_bytes).sum::<u64>() + map.omitted_allocated_bytes
    );
    assert!(f.engine.directory_breakdown(&f.artifact(), 10).is_err());
    assert!(
        f.engine
            .directory_breakdown(f.root.to_str().unwrap(), 201)
            .is_err()
    );
}

#[test]
fn category_rollups_track_incremental_changes_and_republication() {
    let f = Fixture::new();
    f.scan();
    let path = f.root.join("photo.jpg");
    for size in [2048, 4096] {
        fs::write(&path, vec![7; size]).unwrap();
        f.engine
            .reconcile_paths(ReconcileRequest {
                paths: vec![path.display().to_string()],
            })
            .unwrap();
        let categories = f.engine.categories().unwrap();
        let images = categories.iter().find(|c| c.category == "images").unwrap();
        assert_eq!(images.logical_bytes, size as u64);
        assert_eq!(images.files, 1);
    }
    fs::remove_file(&path).unwrap();
    f.engine
        .reconcile_paths(ReconcileRequest {
            paths: vec![path.display().to_string()],
        })
        .unwrap();
    assert!(
        !f.engine
            .categories()
            .unwrap()
            .iter()
            .any(|c| c.category == "images")
    );
    let before = f
        .engine
        .categories()
        .unwrap()
        .iter()
        .map(|c| c.logical_bytes)
        .sum::<u64>();
    f.scan();
    assert_eq!(
        before,
        f.engine
            .categories()
            .unwrap()
            .iter()
            .map(|c| c.logical_bytes)
            .sum::<u64>()
    );
    assert_eq!(f.engine.coverage().unwrap().len(), 1);
}

#[test]
fn developer_insights_suppress_nested_dependencies_and_explain_share() {
    let f = Fixture::new();
    fs::create_dir_all(f.root.join("node_modules/a/node_modules/b")).unwrap();
    fs::write(
        f.root.join("node_modules/a/node_modules/b/index.js"),
        "code",
    )
    .unwrap();
    f.scan();
    let insights = f.engine.insights().unwrap();
    assert_eq!(
        insights
            .iter()
            .filter(|i| i
                .related_resources
                .iter()
                .any(|p| p.ends_with("node_modules")))
            .count(),
        1
    );
    let cargo = insights
        .iter()
        .find(|i| {
            i.related_resources
                .contains(&f.root.join("project/target").display().to_string())
        })
        .unwrap();
    assert!(cargo.measurements.share_of_parent_percent.unwrap() > 0.0);
    assert!(cargo.measurements.parent_logical_bytes.unwrap() > cargo.measurements.logical_bytes);
}

#[test]
fn large_recent_observation_does_not_imply_cleanup_eligibility() {
    let f = Fixture::new();
    let path = f.root.join("important-recording.bin");
    fs::File::create(&path)
        .unwrap()
        .set_len(101 * 1024 * 1024)
        .unwrap();
    f.scan();
    let entry = f.engine.inspect_entry(path.to_str().unwrap()).unwrap();
    let recent: Vec<_> = f
        .engine
        .insights()
        .unwrap()
        .into_iter()
        .filter(|i| i.kind == "recent_large_file")
        .collect();
    if entry.created_at.is_some() {
        assert!(
            recent
                .iter()
                .any(|i| i.related_resources.contains(&path.display().to_string()))
        );
    }
    assert!(
        !f.engine
            .cleanup_candidates(&FileQuery::default())
            .unwrap()
            .items
            .iter()
            .any(|c| c.path == path.display().to_string())
    );
    assert_eq!(
        f.engine
            .create_cleanup_plan(PlanRequest {
                paths: vec![path.display().to_string()]
            })
            .unwrap_err()
            .code,
        "invalid_cleanup_plan"
    );
}

#[test]
fn location_rescan_preserves_exclusions_without_accumulating_duplicates() {
    let f = Fixture::new();
    let ignored = f.root.join("skip");
    fs::create_dir(&ignored).unwrap();
    fs::write(ignored.join("secret"), "skip").unwrap();
    f.engine
        .scan(ScanRequest {
            roots: vec![f.root.display().to_string()],
            exclusions: vec![ignored.display().to_string()],
            ..Default::default()
        })
        .unwrap();
    for _ in 0..3 {
        f.engine.scan_location(f.root.to_str().unwrap()).unwrap();
    }
    let policy = f.engine.scan_policy(f.root.to_str().unwrap()).unwrap();
    assert_eq!(policy.exclusions.len(), 2);
    assert!(
        f.engine
            .inspect_entry(ignored.join("secret").to_str().unwrap())
            .is_err()
    );
}
#[test]
fn staged_generation_is_invisible_until_publication() {
    let f = Fixture::new();
    f.scan();
    let prior = f.engine.files(&FileQuery::default()).unwrap().items.len();
    let record = ScanRecord {
        id: id(),
        root: f.root.to_string_lossy().into(),
        started_at: now(),
        completed_at: None,
        status: "running".into(),
        entries: 0,
        warnings: 0,
        excluded: 0,
        logical_bytes: 0,
        allocated_bytes: 0,
        freshness: "unknown".into(),
    };
    f.store().begin_scan(&record).unwrap();
    assert_eq!(
        f.engine.files(&FileQuery::default()).unwrap().items.len(),
        prior
    );
}
#[test]
fn duplicate_pipeline_excludes_hardlinks_and_invalidates_cache() {
    let f = Fixture::new();
    fs::write(f.root.join("a"), "same-content").unwrap();
    fs::write(f.root.join("b"), "same-content").unwrap();
    fs::write(f.root.join("different"), "diff-content").unwrap();
    fs::hard_link(f.root.join("a"), f.root.join("alias")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(f.root.join("a"), f.root.join("symlink")).unwrap();
    f.scan();
    let report = f
        .engine
        .discover_duplicates(&AtomicBool::new(false))
        .unwrap();
    assert_eq!(report.groups.len(), 1);
    assert_eq!(report.groups[0].files.len(), 2);
    assert_eq!(report.groups[0].reclaimable_size, 12);
    fs::write(f.root.join("b"), "new--content").unwrap();
    f.scan();
    assert!(
        f.engine
            .discover_duplicates(&AtomicBool::new(false))
            .unwrap()
            .groups
            .is_empty()
    );
}
#[test]
fn duplicate_cancellation_is_explicit() {
    let f = Fixture::new();
    fs::write(f.root.join("a"), "same-content").unwrap();
    fs::write(f.root.join("b"), "same-content").unwrap();
    f.scan();
    assert!(
        f.engine
            .discover_duplicates(&AtomicBool::new(true))
            .unwrap()
            .cancelled
    );
}
#[test]
fn quarantine_requires_approval_is_single_use_and_undo_preserves_bytes() {
    let f = Fixture::new();
    f.scan();
    let p = f.plan();
    assert!(PathBuf::from(f.artifact()).exists());
    assert_eq!(
        f.engine
            .execute_cleanup_plan(&p.id, "yes")
            .unwrap_err()
            .code,
        "approval_required"
    );
    let op = f
        .engine
        .execute_cleanup_plan(&p.id, &p.approval_phrase)
        .unwrap();
    assert_eq!(op.status, "completed");
    assert!(!PathBuf::from(f.artifact()).exists());
    assert_eq!(
        fs::read_to_string(&op.items[0].destination).unwrap(),
        "build artifact contents"
    );
    assert_eq!(
        f.engine
            .execute_cleanup_plan(&p.id, &p.approval_phrase)
            .unwrap_err()
            .code,
        "conflict"
    );
    let restored = f.engine.undo_cleanup(&op.id).unwrap();
    assert_eq!(restored.status, "restored");
    assert_eq!(
        fs::read_to_string(f.artifact()).unwrap(),
        "build artifact contents"
    );
    assert!(
        f.store()
            .audit_records(100, 0)
            .unwrap()
            .iter()
            .any(|a| a.action == "restore_completed")
    );
}
#[test]
fn changed_file_invalidates_entire_plan() {
    let f = Fixture::new();
    f.scan();
    let p = f.plan();
    fs::write(f.artifact(), "changed").unwrap();
    assert_eq!(
        f.engine
            .execute_cleanup_plan(&p.id, &p.approval_phrase)
            .unwrap_err()
            .code,
        "filesystem_changed"
    );
    assert_eq!(fs::read_to_string(f.artifact()).unwrap(), "changed");
}
#[test]
fn undo_never_overwrites_recreated_original() {
    let f = Fixture::new();
    f.scan();
    let p = f.plan();
    let op = f
        .engine
        .execute_cleanup_plan(&p.id, &p.approval_phrase)
        .unwrap();
    fs::write(f.artifact(), "new build").unwrap();
    let restored = f.engine.undo_cleanup(&op.id).unwrap();
    assert_eq!(restored.status, "restore_partial");
    assert_eq!(fs::read_to_string(f.artifact()).unwrap(), "new build");
    assert_eq!(
        fs::read_to_string(&op.items[0].destination).unwrap(),
        "build artifact contents"
    );
}
#[test]
fn roots_sources_directories_and_wildcards_are_not_cleanup_candidates() {
    let f = Fixture::new();
    f.scan();
    for path in [
        "/".into(),
        f.root.to_string_lossy().into(),
        f.root.join("project/Cargo.toml").to_string_lossy().into(),
        f.root.join("project/target").to_string_lossy().into(),
        f.root.join("project/target/*").to_string_lossy().into(),
    ] {
        assert!(
            f.engine
                .create_cleanup_plan(PlanRequest { paths: vec![path] })
                .is_err()
        );
    }
}
#[cfg(unix)]
#[test]
fn replacing_parent_with_symlink_cannot_redirect_cleanup() {
    let f = Fixture::new();
    f.scan();
    let p = f.plan();
    let target = f.root.join("project/target");
    let moved = f.root.join("moved");
    fs::rename(&target, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &target).unwrap();
    assert!(
        f.engine
            .execute_cleanup_plan(&p.id, &p.approval_phrase)
            .is_err()
    );
    assert!(moved.join("debug/artifact").exists());
}
#[test]
fn protected_configuration_blocks_even_known_artifacts() {
    let f = Fixture::new();
    f.scan();
    let mut config = f.engine.config.clone();
    config.protected_paths.push(f.root.join("project"));
    let e = Engine::open(config).unwrap();
    assert_eq!(
        e.create_cleanup_plan(PlanRequest {
            paths: vec![f.artifact()]
        })
        .unwrap_err()
        .code,
        "protected_path"
    );
}
#[test]
fn app_footprints_require_exact_evidence() {
    let f = Fixture::new();
    let app = f.root.join("Applications/Example.app/Contents");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("Info.plist"),r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>org.example.app</string><key>CFBundleName</key><string>Example</string></dict></plist>"#).unwrap();
    let cache = f.root.join("Library/Caches/org.example.app");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("data"), vec![1; 1024]).unwrap();
    f.scan();
    let apps = f.engine.applications().unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].associations.len(), 2);
    assert_eq!(apps[0].associations[1].confidence, "high");
    assert!(apps[0].footprint_bytes >= 1024);
}
#[test]
fn overlapping_roots_rejected_to_prevent_double_counting() {
    let f = Fixture::new();
    f.scan();
    assert_eq!(
        f.engine
            .scan(ScanRequest {
                roots: vec![f.root.join("project").to_string_lossy().into()],
                ..Default::default()
            })
            .unwrap_err()
            .code,
        "overlapping_root"
    );
}

#[test]
fn incremental_create_modify_delete_updates_all_ancestors() {
    let f = Fixture::new();
    f.scan();
    let original = f
        .store()
        .entry(f.root.to_str().unwrap())
        .unwrap()
        .logical_bytes;
    let path = f.root.join("project/new.txt");
    fs::write(&path, "12345").unwrap();
    let change = || ReconcileRequest {
        paths: vec![path.to_string_lossy().into()],
    };
    let report = f.engine.reconcile_paths(change()).unwrap();
    assert_eq!(report.updated_paths, 1);
    assert!(report.roots_requiring_scan.is_empty());
    assert_eq!(
        f.store()
            .entry(f.root.to_str().unwrap())
            .unwrap()
            .logical_bytes,
        original + 5
    );
    fs::write(&path, "1234567890").unwrap();
    f.engine.reconcile_paths(change()).unwrap();
    assert_eq!(
        f.store()
            .entry(f.root.to_str().unwrap())
            .unwrap()
            .logical_bytes,
        original + 10
    );
    fs::remove_file(&path).unwrap();
    f.engine.reconcile_paths(change()).unwrap();
    assert_eq!(
        f.store()
            .entry(f.root.to_str().unwrap())
            .unwrap()
            .logical_bytes,
        original
    );
}
#[test]
fn incremental_directory_change_requires_full_reconciliation() {
    let f = Fixture::new();
    f.scan();
    let path = f.root.join("new-directory");
    fs::create_dir(&path).unwrap();
    let report = f
        .engine
        .reconcile_paths(ReconcileRequest {
            paths: vec![path.to_string_lossy().into()],
        })
        .unwrap();
    assert_eq!(report.roots_requiring_scan, vec![f.root.to_string_lossy()]);
}
#[test]
fn data_directory_must_be_dedicated() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("valuable"), "preserve").unwrap();
    let result = Engine::open(Config {
        data_dir: temp.path().to_owned(),
        ..Default::default()
    });
    assert_eq!(result.err().unwrap().code, "protected_path");
    assert_eq!(
        fs::read_to_string(temp.path().join("valuable")).unwrap(),
        "preserve"
    );
}
#[test]
fn tampered_quarantine_is_not_restored() {
    let f = Fixture::new();
    f.scan();
    let p = f.plan();
    let op = f
        .engine
        .execute_cleanup_plan(&p.id, &p.approval_phrase)
        .unwrap();
    fs::write(&op.items[0].destination, "changed quarantine").unwrap();
    let undone = f.engine.undo_cleanup(&op.id).unwrap();
    assert_eq!(undone.status, "restore_partial");
    assert!(!PathBuf::from(f.artifact()).exists());
}
#[test]
fn expired_plan_fails_without_moving_files() {
    let f = Fixture::new();
    f.scan();
    let mut p = f.plan();
    p.expires_at = 0;
    f.store().put("plan", &p.id, &p).unwrap();
    assert_eq!(
        f.engine
            .execute_cleanup_plan(&p.id, &p.approval_phrase)
            .unwrap_err()
            .code,
        "invalid_cleanup_plan"
    );
    assert!(PathBuf::from(f.artifact()).exists());
}
#[test]
fn undo_recovers_a_rename_completed_before_journal_commit() {
    let f = Fixture::new();
    f.scan();
    let p = f.plan();
    let mut op = f
        .engine
        .execute_cleanup_plan(&p.id, &p.approval_phrase)
        .unwrap();
    op.status = "running".into();
    op.items[0].status = "moving".into();
    f.store().put("operation", &op.id, &op).unwrap();
    let reopened = Engine::open(f.engine.config.clone()).unwrap();
    let restored = reopened.undo_cleanup(&op.id).unwrap();
    assert_eq!(restored.status, "restored");
    assert!(PathBuf::from(f.artifact()).is_file());
}
#[test]
fn undo_recovers_a_restore_completed_before_journal_commit() {
    let f = Fixture::new();
    f.scan();
    let p = f.plan();
    let op = f
        .engine
        .execute_cleanup_plan(&p.id, &p.approval_phrase)
        .unwrap();
    stratum_platform::secure_fs::rename_no_replace(
        std::path::Path::new(&op.items[0].destination),
        std::path::Path::new(&op.items[0].source),
    )
    .unwrap();
    let restored = f.engine.undo_cleanup(&op.id).unwrap();
    assert_eq!(restored.status, "restored");
    assert_eq!(
        fs::read_to_string(f.artifact()).unwrap(),
        "build artifact contents"
    );
}
#[test]
fn failed_scan_does_not_replace_published_policy() {
    let f = Fixture::new();
    f.scan();
    let before = f.engine.scan_policy(f.root.to_str().unwrap()).unwrap();
    let result = f.engine.scan(ScanRequest {
        roots: vec![f.root.to_string_lossy().into()],
        ignore_patterns: vec!["[".into()],
        ..Default::default()
    });
    assert!(result.is_err());
    assert_eq!(
        f.engine
            .scan_policy(f.root.to_str().unwrap())
            .unwrap()
            .ignore_patterns,
        before.ignore_patterns
    );
}

#[test]
fn live_view_shows_a_first_scan_while_it_runs_and_keeps_published_data_during_rescans() {
    let f = Fixture::new();
    f.engine.set_live_view(true);
    let store = f.store();
    let root = f.root.to_string_lossy().to_string();
    let running = ScanRecord {
        id: id(),
        root: root.clone(),
        started_at: now(),
        completed_at: None,
        status: "running".into(),
        entries: 0,
        warnings: 0,
        excluded: 0,
        logical_bytes: 0,
        allocated_bytes: 0,
        freshness: "unknown".into(),
    };
    store.begin_scan(&running).unwrap();
    let entry =
        stratum_platform::scanner::read_entry(&f.root.join("project/Cargo.toml"), 2).unwrap();
    store.insert_batch(&running.id, &[entry]).unwrap();
    assert_eq!(
        f.engine.files(&FileQuery::default()).unwrap().items.len(),
        1
    );
    assert_eq!(f.engine.coverage().unwrap()[0].status, "running");
    let mut cancelled = running.clone();
    cancelled.status = "cancelled".into();
    cancelled.completed_at = Some(now());
    store.finish_scan(&cancelled, 30).unwrap();
    assert!(
        f.engine
            .files(&FileQuery::default())
            .unwrap()
            .items
            .is_empty()
    );
    f.scan();
    let published = f.engine.files(&FileQuery::default()).unwrap().items.len();
    assert!(published > 1);
    let rescan = ScanRecord {
        id: id(),
        ..running
    };
    store.begin_scan(&rescan).unwrap();
    store
        .insert_batch(
            &rescan.id,
            &[
                stratum_platform::scanner::read_entry(&f.root.join("project/Cargo.toml"), 2)
                    .unwrap(),
            ],
        )
        .unwrap();
    assert_eq!(
        f.engine.files(&FileQuery::default()).unwrap().items.len(),
        published,
        "a rescan stays invisible until it publishes"
    );
    assert_eq!(f.engine.coverage().unwrap()[0].status, "completed");
}

#[test]
fn derived_views_are_cached_until_the_index_changes() {
    let f = Fixture::new();
    f.scan();
    let first = f.engine.explain_storage().unwrap();
    let again = f.engine.explain_storage().unwrap();
    assert_eq!(
        serde_json::to_string(&first.insights).unwrap(),
        serde_json::to_string(&again.insights).unwrap()
    );
    assert_eq!(
        first.largest_directories.len(),
        again.largest_directories.len()
    );
    fs::write(f.root.join("project/target/debug/second"), vec![7; 4096]).unwrap();
    f.scan();
    let after = f.engine.explain_storage().unwrap();
    assert_eq!(
        after.largest_directories[0].logical_bytes,
        first.largest_directories[0].logical_bytes + 4096
    );
}

#[test]
fn scans_stream_progress_and_directory_totals_match_their_files() {
    let f = Fixture::new();
    let mut events = f.engine.subscribe();
    let records = f.scan();
    assert_eq!(records[0].status, "completed");
    let (mut progress, mut completed) = (0, false);
    while let Ok(event) = events.try_recv() {
        match event {
            OperationEvent::ScanProgress { .. } => progress += 1,
            OperationEvent::ScanCompleted { .. } => completed = true,
            _ => {}
        }
    }
    assert!(progress >= 1 && completed);
    let files: u64 = f
        .engine
        .files(&FileQuery {
            kind: Some("file".into()),
            limit: 1000,
            ..Default::default()
        })
        .unwrap()
        .items
        .iter()
        .map(|e| e.logical_bytes)
        .sum();
    assert!(files > 0);
    let root = f.engine.inspect_entry(f.root.to_str().unwrap()).unwrap();
    let project = f
        .engine
        .inspect_entry(f.root.join("project").to_str().unwrap())
        .unwrap();
    assert_eq!(root.logical_bytes, files);
    assert_eq!(project.logical_bytes, files);
    assert_eq!(records[0].entries, 6);
}
