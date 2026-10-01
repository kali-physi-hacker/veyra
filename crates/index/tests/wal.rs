//! The write-ahead log never outlives the scan that grew it.
use stratum_domain::*;
use stratum_index::Store;

fn entry(path: &str, bytes: u64) -> Entry {
    Entry {
        path: path.into(),
        parent: "/root".into(),
        name: path.rsplit('/').next().unwrap().into(),
        kind: EntryKind::File,
        logical_bytes: bytes,
        allocated_bytes: bytes,
        modified_at: Some(1),
        created_at: Some(1),
        accessed_at: None,
        extension: String::new(),
        category: "documents".into(),
        confidence: 0.5,
        evidence: vec![Evidence::new("path_classification", "test")],
        identity: Identity { device: 1, inode: bytes, size: bytes, modified_ns: 0, changed_ns: 0, links: 1 },
        depth: 1,
    }
}
fn record(id: &str) -> ScanRecord {
    ScanRecord {
        id: id.into(),
        root: "/root".into(),
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
/// Streams `batches` batches of a thousand entries in bulk mode, the way a scan does.
fn stream(store: &Store, id: &str, batches: u64) {
    store.begin_scan(&record(id)).unwrap();
    for b in 0..batches {
        let batch: Vec<Entry> = (0..1000).map(|i| entry(&format!("/root/{id}-{b:04}-{i:04}-{}", "x".repeat(60)), b * 1000 + i + 1)).collect();
        store.insert_batch(id, &batch).unwrap();
    }
}

#[test]
fn a_finished_scan_leaves_no_write_ahead_log() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("db")).unwrap();
    store.set_bulk(true).unwrap();
    stream(&store, "s1", 20);
    assert!(store.wal_bytes() > 1 << 20, "the scan should have grown the WAL, got {}", store.wal_bytes());
    let mut done = record("s1");
    done.status = "completed".into();
    done.completed_at = Some(now());
    store.finish_scan(&done, 30).unwrap();
    store.set_bulk(false).unwrap();
    assert_eq!(store.wal_bytes(), 0, "the WAL should be truncated once the scan ends");
    assert_eq!(store.roots().unwrap(), vec!["/root".to_string()]);
}

#[test]
fn a_write_ahead_log_left_behind_is_folded_in_on_open() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    // A first process grows the WAL and stays open, as an app interrupted mid-scan would leave it.
    let first = Store::open(&path).unwrap();
    first.set_bulk(true).unwrap();
    stream(&first, "s1", 10);
    let left = first.wal_bytes();
    assert!(left > 1 << 20, "expected a WAL to be left, got {left}");
    // Opening the index again folds it into the database and truncates it, with the first
    // store still holding its connections, and nothing written is lost.
    let second = Store::open(&path).unwrap();
    assert_eq!(second.wal_bytes(), 0, "a {left}-byte WAL should be truncated on open");
    let mut done = record("s1");
    done.status = "completed".into();
    done.completed_at = Some(now());
    first.finish_scan(&done, 30).unwrap();
    first.set_bulk(false).unwrap();
    assert_eq!(second.roots().unwrap(), vec!["/root".to_string()]);
}
