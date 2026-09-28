//! Visible generations and concurrent readers.
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use stratum_domain::*;
use stratum_index::Store;

fn entry(path: &str, parent: &str, kind: EntryKind, bytes: u64, category: &str) -> Entry {
    Entry {
        path: path.into(),
        parent: parent.into(),
        name: path.rsplit('/').next().unwrap().into(),
        kind,
        logical_bytes: bytes,
        allocated_bytes: bytes,
        modified_at: Some(1),
        created_at: Some(1),
        accessed_at: None,
        extension: String::new(),
        category: category.into(),
        confidence: 0.5,
        evidence: vec![Evidence::new("path_classification", "test")],
        identity: Identity {
            device: 1,
            inode: bytes,
            size: bytes,
            modified_ns: 0,
            changed_ns: 0,
            links: 1,
        },
        depth: path.matches('/').count() as u32 - 1,
    }
}
fn record(id: &str, root: &str) -> ScanRecord {
    ScanRecord {
        id: id.into(),
        root: root.into(),
        started_at: now(),
        completed_at: None,
        status: "running".into(),
        entries: 0,
        warnings: 0,
        excluded: 0,
        logical_bytes: 0,
        allocated_bytes: 0,
        freshness: "unknown".into(),
    }
}

#[test]
fn a_first_scan_is_visible_while_running_and_a_rescan_is_not() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("db")).unwrap();
    store.begin_scan(&record("first", "/root")).unwrap();
    store
        .insert_batch(
            "first",
            &[
                entry("/root", "/", EntryKind::Directory, 0, "other"),
                entry("/root/a.log", "/root", EntryKind::File, 10, "logs"),
            ],
        )
        .unwrap();
    assert!(store.files(&FileQuery::default()).unwrap().items.is_empty());
    assert!(store.categories().unwrap().is_empty());
    assert!(store.published_scans().unwrap().is_empty());

    store.set_live_view(true);
    let files = store.files(&FileQuery::default()).unwrap().items;
    assert_eq!(files.len(), 2);
    assert_eq!(store.entry("/root/a.log").unwrap().logical_bytes, 10);
    let categories = store.categories().unwrap();
    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0].category, "logs");
    assert_eq!(categories[0].logical_bytes, 10);
    assert_eq!(store.published_scans().unwrap()[0].status, "running");
    assert_eq!(
        store.directory_breakdown("/root", 10).unwrap().child_count,
        1
    );

    let mut finished = record("first", "/root");
    finished.status = "completed".into();
    finished.completed_at = Some(now());
    finished.entries = 2;
    store.finish_scan(&finished, 30).unwrap();
    assert_eq!(store.roots().unwrap(), vec!["/root".to_string()]);

    // The rescan keeps the published generation visible until it publishes.
    store.begin_scan(&record("second", "/root")).unwrap();
    store
        .insert_batch(
            "second",
            &[entry("/root/b.log", "/root", EntryKind::File, 99, "logs")],
        )
        .unwrap();
    let visible = store.files(&FileQuery::default()).unwrap().items;
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().all(|e| e.path != "/root/b.log"));
    assert_eq!(store.categories().unwrap()[0].logical_bytes, 10);
    assert_eq!(store.published_scans().unwrap()[0].id, "first");

    store.set_live_view(false);
    assert_eq!(store.files(&FileQuery::default()).unwrap().items.len(), 2);
}

#[test]
fn rows_go_in_path_order_and_provisional_directories_are_replaced() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("db")).unwrap();
    store.set_live_view(true);
    store.begin_scan(&record("scan", "/root")).unwrap();
    let mut provisional = entry("/root", "/", EntryKind::Directory, 0, "other");
    provisional.logical_bytes = 5;
    store
        .insert_batch(
            "scan",
            &[
                entry("/root/z", "/root", EntryKind::File, 5, "other"),
                provisional,
                entry("/root/a", "/root", EntryKind::File, 7, "other"),
            ],
        )
        .unwrap();
    assert_eq!(store.entry("/root").unwrap().logical_bytes, 5);
    let mut complete = entry("/root", "/", EntryKind::Directory, 0, "other");
    complete.logical_bytes = 12;
    store.insert_batch("scan", &[complete]).unwrap();
    assert_eq!(store.entry("/root").unwrap().logical_bytes, 12);
    let ordered = store
        .files(&FileQuery {
            sort: "path".into(),
            ..Default::default()
        })
        .unwrap()
        .items;
    assert_eq!(
        ordered.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
        vec!["/root", "/root/a", "/root/z"]
    );
    assert_eq!(store.categories().unwrap()[0].files, 2);
}

#[test]
fn readers_do_not_wait_for_a_long_write() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&temp.path().join("db")).unwrap());
    store.begin_scan(&record("scan", "/root")).unwrap();
    let mut first = record("scan", "/root");
    store
        .insert_batch(
            "scan",
            &[entry("/root", "/", EntryKind::Directory, 0, "other")],
        )
        .unwrap();
    first.status = "completed".into();
    first.completed_at = Some(now());
    store.finish_scan(&first, 30).unwrap();
    store.begin_scan(&record("big", "/root")).unwrap();
    let batch: Vec<Entry> = (0..60_000)
        .map(|i| {
            entry(
                &format!("/root/dir{}/file-{i:06}.bin", i % 97),
                &format!("/root/dir{}", i % 97),
                EntryKind::File,
                i as u64 + 1,
                "other",
            )
        })
        .collect();
    let writing = Arc::new(AtomicBool::new(true));
    let writer = {
        let (store, writing) = (store.clone(), writing.clone());
        std::thread::spawn(move || {
            store.insert_batch("big", &batch).unwrap();
            writing.store(false, Ordering::SeqCst);
        })
    };
    std::thread::sleep(Duration::from_millis(30));
    let started = std::time::Instant::now();
    let roots = store.roots().unwrap();
    let published = store.entry("/root").unwrap();
    assert_eq!(roots, vec!["/root".to_string()]);
    assert_eq!(published.kind, EntryKind::Directory);
    assert!(
        writing.load(Ordering::SeqCst),
        "the reads should have finished while the batch was still being written"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    writer.join().unwrap();
}
