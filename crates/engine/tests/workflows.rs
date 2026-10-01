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
            .create_cleanup_plan(PlanRequest::of_files(vec![self.artifact()]))
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
            .create_cleanup_plan(PlanRequest::of_files(vec![path.display().to_string()]))
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
                .create_cleanup_plan(PlanRequest::of_files(vec![path]))
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
        e.create_cleanup_plan(PlanRequest::of_files(vec![f.artifact()]))
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

#[test]
fn cleanup_locations_cover_the_whole_index_not_one_page_of_large_files() {
    let f = Fixture::new();
    // A large file elsewhere fills the largest-files page; it must not hide the candidates.
    fs::write(f.root.join("movie.mov"), vec![0u8; 256 * 1024]).unwrap();
    // A folder called target without a Cargo.toml beside it is not Cargo build output.
    fs::create_dir_all(f.root.join("maven/target")).unwrap();
    fs::write(f.root.join("maven/target/app.jar"), "jar").unwrap();
    // The npm content cache and Cargo's registry cache.
    fs::create_dir_all(f.root.join("home/.npm/_cacache/content-v2")).unwrap();
    fs::write(
        f.root.join("home/.npm/_cacache/content-v2/blob"),
        "npm blob",
    )
    .unwrap();
    fs::create_dir_all(f.root.join("home/.cargo/registry/cache/index.crates.io")).unwrap();
    fs::write(
        f.root
            .join("home/.cargo/registry/cache/index.crates.io/serde.crate"),
        "crate",
    )
    .unwrap();
    // A target nested inside another target is counted in the outer one.
    fs::create_dir_all(f.root.join("project/target/debug/build/x/target")).unwrap();
    fs::write(
        f.root.join("project/target/debug/build/x/Cargo.toml"),
        "[package]\nname='x'\n",
    )
    .unwrap();
    fs::write(
        f.root.join("project/target/debug/build/x/target/out"),
        "nested",
    )
    .unwrap();
    f.scan();

    let root = f.root.to_string_lossy().to_string();
    let found = f.engine.cleanup_locations(None).unwrap();
    let mut paths: Vec<String> = found
        .iter()
        .map(|l| l.path.trim_start_matches(&root).to_string())
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![
            "/home/.cargo/registry/cache",
            "/home/.npm/_cacache",
            "/project/target"
        ],
        "{found:?}"
    );
    let cargo = found
        .iter()
        .find(|l| l.path.ends_with("/project/target"))
        .unwrap();
    assert_eq!(cargo.category, "developer_build_artifact");
    assert!(
        cargo.logical_bytes > 0,
        "a folder carries its indexed total"
    );
    assert!(
        found
            .windows(2)
            .all(|w| w[0].logical_bytes >= w[1].logical_bytes),
        "largest first"
    );

    // A scope keeps only the folders inside it, and the folder's files are candidates.
    let project = f.root.join("project").to_string_lossy().to_string();
    let scoped = f.engine.cleanup_locations(Some(&project)).unwrap();
    assert_eq!(scoped.len(), 1, "{scoped:?}");
    let files = f
        .engine
        .cleanup_candidates(&FileQuery {
            path: Some(scoped[0].path.clone()),
            limit: 100,
            ..Default::default()
        })
        .unwrap();
    assert!(
        files.items.iter().any(|c| c.path == f.artifact()),
        "{:?}",
        files.items
    );
}
/// Quarantines freshly written Cargo artifacts under the fixture's target folder.
fn quarantined(f: &Fixture, names: &[&str]) -> CleanupOperation {
    let debug = f.root.join("project/target/debug");
    for name in names {
        fs::write(debug.join(name), format!("{name} generated bytes")).unwrap();
    }
    f.scan();
    let paths = names
        .iter()
        .map(|n| debug.join(n).to_string_lossy().into_owned())
        .collect();
    let p = f
        .engine
        .create_cleanup_plan(PlanRequest::of_files(paths))
        .unwrap();
    let op = f
        .engine
        .execute_cleanup_plan(&p.id, &p.approval_phrase)
        .unwrap();
    assert_eq!(op.status, "completed");
    op
}
fn item<'a>(op: &'a CleanupOperation, name: &str) -> &'a QuarantineItem {
    op.items
        .iter()
        .find(|i| i.source.ends_with(&format!("/{name}")))
        .unwrap()
}
#[test]
fn purge_needs_its_own_phrase_and_deletes_only_what_quarantine_holds() {
    let f = Fixture::new();
    let op = quarantined(&f, &["a1", "b2", "c3"]);
    let quarantine = PathBuf::from(&op.items[0].destination)
        .parent()
        .unwrap()
        .to_owned();
    for approval in [String::from("yes"), format!("QUARANTINE {}", op.plan_id)] {
        assert_eq!(
            f.engine
                .purge_quarantine(&op.id, &approval)
                .unwrap_err()
                .code,
            "approval_required"
        );
    }
    assert!(
        op.items
            .iter()
            .all(|i| PathBuf::from(&i.destination).is_file())
    );
    let held = op.purgeable_bytes();
    assert!(held > 0);
    let purged = f
        .engine
        .purge_quarantine(&op.id, &purge_phrase(&op.id))
        .unwrap();
    assert_eq!(purged.status, "purged");
    assert!(purged.items.iter().all(|i| i.status == "purged"));
    assert!(purged.purged_at.is_some());
    assert_eq!(purged.purged_bytes(), held);
    for i in &purged.items {
        assert!(!PathBuf::from(&i.destination).exists());
        assert!(!PathBuf::from(&i.source).exists(), "a purge never restores");
    }
    assert!(
        !quarantine.exists(),
        "the emptied operation folder goes too"
    );
    assert_eq!(f.engine.cleanup_operation(&op.id).unwrap().status, "purged");
    assert_eq!(f.engine.undo_cleanup(&op.id).unwrap_err().code, "conflict");
    assert_eq!(
        f.engine
            .purge_quarantine(&op.id, &purge_phrase(&op.id))
            .unwrap_err()
            .code,
        "conflict"
    );
    let audit = f.store().audit_records(100, 0).unwrap();
    let count = |action: &str| audit.iter().filter(|a| a.action == action).count();
    assert_eq!(count("purge_started"), 1);
    assert_eq!(count("purge_item"), 3);
    assert_eq!(count("purge_completed"), 1);
}
#[test]
fn purge_leaves_a_changed_quarantine_file_and_restore_skips_what_is_gone() {
    let f = Fixture::new();
    let op = quarantined(&f, &["a1", "b2"]);
    let changed = item(&op, "a1").clone();
    fs::write(&changed.destination, "replaced inside quarantine").unwrap();
    let purged = f
        .engine
        .purge_quarantine(&op.id, &purge_phrase(&op.id))
        .unwrap();
    assert_eq!(purged.status, "purge_partial");
    assert_eq!(item(&purged, "a1").status, "purge_failed");
    assert!(item(&purged, "a1").error.is_some());
    assert_eq!(item(&purged, "b2").status, "purged");
    assert_eq!(
        fs::read_to_string(&changed.destination).unwrap(),
        "replaced inside quarantine",
        "a file that no longer matches is never deleted"
    );
    let undone = f.engine.undo_cleanup(&op.id).unwrap();
    assert_eq!(undone.status, "restore_partial");
    assert_eq!(item(&undone, "a1").status, "restore_failed");
    assert_eq!(item(&undone, "b2").status, "purged");
    assert!(!PathBuf::from(&item(&undone, "b2").source).exists());
}
#[test]
fn purge_waits_for_the_configured_time_in_quarantine() {
    let f = Fixture::new();
    let op = quarantined(&f, &["a1"]);
    let mut config = f.engine.config.clone();
    config.purge_after_hours = 2;
    let waiting = Engine::open(config).unwrap();
    let error = waiting
        .purge_quarantine(&op.id, &purge_phrase(&op.id))
        .unwrap_err();
    assert_eq!(error.code, "conflict");
    assert!(error.message.contains("ready in 2 h"), "{}", error.message);
    assert!(PathBuf::from(&op.items[0].destination).is_file());
    let mut aged = f.engine.cleanup_operation(&op.id).unwrap();
    aged.created_at -= 3 * 3600;
    f.store().put("operation", &op.id, &aged).unwrap();
    let purged = waiting
        .purge_quarantine(&op.id, &purge_phrase(&op.id))
        .unwrap();
    assert_eq!(purged.status, "purged");
}
#[test]
fn an_interrupted_purge_completes_and_outside_removals_are_reported() {
    let f = Fixture::new();
    let mut op = quarantined(&f, &["a1", "b2", "c3"]);
    // a1 was deleted just before a crash; c3 was still waiting; b2 was removed by hand.
    for item in &mut op.items {
        if !item.source.ends_with("/b2") {
            item.status = "purging".into();
        }
    }
    op.status = "purging".into();
    f.store().put("operation", &op.id, &op).unwrap();
    fs::remove_file(&item(&op, "a1").destination).unwrap();
    fs::remove_file(&item(&op, "b2").destination).unwrap();
    let purged = f
        .engine
        .purge_quarantine(&op.id, &purge_phrase(&op.id))
        .unwrap();
    assert_eq!(purged.status, "purged");
    assert_eq!(item(&purged, "a1").status, "purged");
    assert_eq!(item(&purged, "b2").status, "missing");
    assert!(
        item(&purged, "b2")
            .error
            .as_ref()
            .unwrap()
            .contains("outside Stratum")
    );
    assert_eq!(item(&purged, "c3").status, "purged");
    assert!(!PathBuf::from(&item(&purged, "c3").destination).exists());
    assert_eq!(f.engine.undo_cleanup(&op.id).unwrap_err().code, "conflict");
}
#[test]
fn purge_never_deletes_outside_the_operation_quarantine_folder() {
    let f = Fixture::new();
    let mut op = quarantined(&f, &["a1"]);
    let outside = f.root.join("project/Cargo.toml");
    let (hash, identity) =
        stratum_platform::secure_fs::hash_file(&outside, false, || false).unwrap();
    op.items[0].destination = outside.to_string_lossy().into();
    op.items[0].hash = hash;
    op.items[0].identity = identity;
    f.store().put("operation", &op.id, &op).unwrap();
    let purged = f
        .engine
        .purge_quarantine(&op.id, &purge_phrase(&op.id))
        .unwrap();
    assert_eq!(purged.items[0].status, "purge_failed");
    assert!(purged.items[0].error.as_ref().unwrap().contains("outside"));
    assert!(outside.is_file());
}
/// Adds build output beside the fixture's artifact and scans; returns the target folder.
fn target_folder(f: &Fixture) -> String {
    let target = f.root.join("project/target");
    fs::create_dir_all(target.join("debug/deps")).unwrap();
    fs::create_dir_all(target.join("release")).unwrap();
    fs::write(target.join("debug/deps/a.rlib"), vec![1u8; 3000]).unwrap();
    fs::write(target.join("debug/deps/b.rlib"), vec![2u8; 2000]).unwrap();
    fs::write(target.join("release/app"), vec![3u8; 1000]).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        f.root.join("project/Cargo.toml"),
        target.join("manifest-link"),
    )
    .unwrap();
    f.scan();
    target.to_string_lossy().into_owned()
}
#[test]
fn a_whole_folder_moves_in_one_step_leaves_the_index_and_comes_back() {
    let f = Fixture::new();
    let target = target_folder(&f);
    assert!(
        f.engine
            .cleanup_locations(None)
            .unwrap()
            .iter()
            .any(|l| l.path == target)
    );
    let project_before = f
        .engine
        .inspect_entry(&f.root.join("project").to_string_lossy())
        .unwrap();
    let plan = f
        .engine
        .create_cleanup_plan(PlanRequest::of_folders(vec![target.clone()]))
        .unwrap();
    assert_eq!(plan.action, "quarantine_folders");
    assert_eq!(plan.delete_phrase, format!("DELETE {}", plan.id));
    let folder = plan.items[0]
        .folder
        .expect("a folder item carries its contents");
    assert_eq!(folder.directories, 3, "debug, debug/deps and release");
    assert_eq!(folder.files, 5, "four files and a symlink");
    assert!(plan.items[0].bytes >= 6000 + "build artifact contents".len() as u64);
    let op = f
        .engine
        .execute_cleanup_plan(&plan.id, &plan.approval_phrase)
        .unwrap();
    assert_eq!(op.status, "completed");
    assert!(!PathBuf::from(&target).exists());
    let held = PathBuf::from(&op.items[0].destination);
    assert_eq!(fs::read(held.join("release/app")).unwrap(), vec![3u8; 1000]);
    // The index no longer lists the folder or anything in it, and its parent shrank.
    assert_eq!(
        f.engine.inspect_entry(&target).unwrap_err().code,
        "path_not_found"
    );
    assert_eq!(
        f.engine.inspect_entry(&f.artifact()).unwrap_err().code,
        "path_not_found"
    );
    let project_after = f
        .engine
        .inspect_entry(&f.root.join("project").to_string_lossy())
        .unwrap();
    assert!(project_after.logical_bytes + 6000 <= project_before.logical_bytes);
    assert!(f.engine.cleanup_locations(None).unwrap().is_empty());
    let restored = f.engine.undo_cleanup(&op.id).unwrap();
    assert_eq!(restored.status, "restored");
    assert_eq!(
        fs::read(PathBuf::from(&target).join("release/app")).unwrap(),
        vec![3u8; 1000]
    );
    assert_eq!(
        fs::read_to_string(f.artifact()).unwrap(),
        "build artifact contents"
    );
}
#[test]
fn the_delete_phrase_removes_a_whole_folder_in_one_operation() {
    let f = Fixture::new();
    let target = target_folder(&f);
    let plan = f
        .engine
        .create_cleanup_plan(PlanRequest::of_folders(vec![target.clone()]))
        .unwrap();
    assert_eq!(
        f.engine
            .execute_cleanup_plan(&plan.id, &purge_phrase(&plan.id))
            .unwrap_err()
            .code,
        "approval_required",
        "a purge phrase never runs a plan"
    );
    let op = f
        .engine
        .execute_cleanup_plan(&plan.id, &plan.delete_phrase)
        .unwrap();
    assert_eq!(op.status, "purged");
    assert_eq!(op.items[0].status, "purged");
    assert_eq!(op.purged_bytes(), plan.total_bytes);
    assert!(!PathBuf::from(&target).exists());
    assert!(!PathBuf::from(&op.items[0].destination).exists());
    assert!(
        f.root.join("project/Cargo.toml").is_file(),
        "a symlink inside the folder is removed, never followed"
    );
    assert_eq!(f.engine.undo_cleanup(&op.id).unwrap_err().code, "conflict");
    let audit = f.store().audit_records(100, 0).unwrap();
    assert!(
        audit
            .iter()
            .any(|a| a.action == "cleanup_started"
                && a.detail.contains("permanent deletion approved"))
    );
    assert!(audit.iter().any(|a| a.action == "purge_completed"));
}
#[test]
fn folder_plans_take_only_whole_recognised_folders() {
    let f = Fixture::new();
    let target = target_folder(&f);
    let plan = |folders: Vec<String>| {
        f.engine
            .create_cleanup_plan(PlanRequest::of_folders(folders))
            .unwrap_err()
            .code
    };
    fs::create_dir_all(f.root.join("project/src")).unwrap();
    fs::create_dir_all(f.root.join("loose/target")).unwrap();
    fs::write(f.root.join("loose/target/x"), "x").unwrap();
    f.scan();
    let path = |p: &str| f.root.join(p).to_string_lossy().into_owned();
    assert_eq!(plan(vec![path("project/src")]), "invalid_cleanup_plan");
    assert_eq!(
        plan(vec![path("loose/target")]),
        "invalid_cleanup_plan",
        "no Cargo.toml beside it"
    );
    assert_eq!(
        plan(vec![f.root.to_string_lossy().into_owned()]),
        "protected_path"
    );
    assert_eq!(
        plan(vec![target.clone(), path("project/target/debug")]),
        "invalid_request"
    );
    let mixed = f.engine.create_cleanup_plan(PlanRequest {
        paths: vec![f.artifact()],
        folders: vec![target.clone()],
    });
    assert_eq!(mixed.unwrap_err().code, "invalid_request");
    fs::create_dir_all(PathBuf::from(&target).join("debug/build/vendored/.git")).unwrap();
    assert_eq!(
        plan(vec![target.clone()]),
        "protected_path",
        "a protected name inside refuses the folder"
    );
}
#[test]
fn a_replaced_folder_invalidates_its_plan_and_a_recreated_one_is_never_overwritten() {
    let f = Fixture::new();
    let target = target_folder(&f);
    let plan = f
        .engine
        .create_cleanup_plan(PlanRequest::of_folders(vec![target.clone()]))
        .unwrap();
    fs::rename(&target, f.root.join("project/old-target")).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(PathBuf::from(&target).join("fresh"), "new build").unwrap();
    assert_eq!(
        f.engine
            .execute_cleanup_plan(&plan.id, &plan.approval_phrase)
            .unwrap_err()
            .code,
        "filesystem_changed"
    );
    assert!(PathBuf::from(&target).join("fresh").is_file());
    assert!(f.root.join("project/old-target/release/app").is_file());
    // Quarantine the new folder, rebuild under the same name, then restore: the rebuild stays.
    f.scan();
    let plan = f
        .engine
        .create_cleanup_plan(PlanRequest::of_folders(vec![target.clone()]))
        .unwrap();
    let op = f
        .engine
        .execute_cleanup_plan(&plan.id, &plan.approval_phrase)
        .unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(PathBuf::from(&target).join("rebuilt"), "rebuilt").unwrap();
    let undone = f.engine.undo_cleanup(&op.id).unwrap();
    assert_eq!(undone.status, "restore_partial");
    assert!(PathBuf::from(&target).join("rebuilt").is_file());
    assert!(
        PathBuf::from(&op.items[0].destination)
            .join("fresh")
            .is_file()
    );
}
#[test]
fn delete_waits_when_time_in_quarantine_is_configured() {
    let f = Fixture::new();
    let target = target_folder(&f);
    let mut config = f.engine.config.clone();
    config.purge_after_hours = 2;
    let waiting = Engine::open(config).unwrap();
    let plan = waiting
        .create_cleanup_plan(PlanRequest::of_folders(vec![target.clone()]))
        .unwrap();
    let error = waiting
        .execute_cleanup_plan(&plan.id, &plan.delete_phrase)
        .unwrap_err();
    assert_eq!(error.code, "conflict");
    assert!(PathBuf::from(&target).is_dir(), "nothing moved");
    let op = waiting
        .execute_cleanup_plan(&plan.id, &plan.approval_phrase)
        .unwrap();
    assert_eq!(op.status, "completed");
}
#[test]
fn quarantined_files_leave_the_index_and_return_with_a_restore() {
    let f = Fixture::new();
    f.scan();
    let listed = |f: &Fixture| {
        f.engine
            .cleanup_candidates(&FileQuery {
                path: Some(f.root.join("project/target").to_string_lossy().into()),
                ..Default::default()
            })
            .unwrap()
            .items
            .iter()
            .any(|c| c.path == f.artifact())
    };
    assert!(listed(&f));
    let p = f.plan();
    let op = f
        .engine
        .execute_cleanup_plan(&p.id, &p.approval_phrase)
        .unwrap();
    assert!(
        !listed(&f),
        "the next selection starts from what is still there"
    );
    f.engine.undo_cleanup(&op.id).unwrap();
    assert!(listed(&f));
}
