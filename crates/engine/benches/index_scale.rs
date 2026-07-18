//! Synthetic metadata, not real user files. Measures millions of SQLite records with bounded batches.
use std::time::Instant;
use stratum_engine::{Engine, domain::*};
fn main() {
    let count = std::env::var("STRATUM_BENCH_FILES")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(100000);
    let temp = tempfile::tempdir().unwrap();
    let engine = Engine::open(Config {
        data_dir: temp.path().join("state"),
        ..Default::default()
    })
    .unwrap();
    let store = stratum_index::Store::open(&engine.config.data_dir.join("index.sqlite3")).unwrap();
    let mut scan = ScanRecord {
        id: id(),
        root: "/synthetic".into(),
        started_at: now(),
        completed_at: None,
        status: "running".into(),
        entries: 0,
        warnings: 0,
        excluded: 0,
        logical_bytes: count * 4096,
        allocated_bytes: count * 4096,
        freshness: "probably_fresh".into(),
    };
    store.begin_scan(&scan).unwrap();
    let start = Instant::now();
    let mut batch = Vec::with_capacity(1000);
    let identity = Identity {
        device: 1,
        inode: 0,
        size: 4096,
        modified_ns: 1,
        changed_ns: 1,
        links: 1,
    };
    for i in 0..count {
        let path = format!("/synthetic/d{:05}/f{i:09}.bin", i / 1000);
        let parent = format!("/synthetic/d{:05}", i / 1000);
        batch.push(Entry {
            path,
            parent,
            name: format!("f{i:09}.bin"),
            kind: EntryKind::File,
            logical_bytes: 4096,
            allocated_bytes: 4096,
            modified_at: Some(1),
            created_at: None,
            accessed_at: None,
            extension: "bin".into(),
            category: "other".into(),
            confidence: 0.0,
            evidence: vec![],
            identity: Identity {
                inode: i + 1,
                ..identity.clone()
            },
            depth: 2,
        });
        if batch.len() == 1000 {
            store.insert_batch(&scan.id, &batch).unwrap();
            batch.clear();
        }
    }
    store.insert_batch(&scan.id, &batch).unwrap();
    batch.clear();
    for d in 0..count.div_ceil(1000) {
        let size = (count - d * 1000).min(1000) * 4096;
        batch.push(Entry {
            path: format!("/synthetic/d{d:05}"),
            parent: "/synthetic".into(),
            name: format!("d{d:05}"),
            kind: EntryKind::Directory,
            logical_bytes: size,
            allocated_bytes: size,
            modified_at: Some(1),
            created_at: None,
            accessed_at: None,
            extension: String::new(),
            category: "other".into(),
            confidence: 0.0,
            evidence: vec![],
            identity: Identity {
                inode: count + d + 1,
                ..identity.clone()
            },
            depth: 1,
        });
        if batch.len() == 1000 {
            store.insert_batch(&scan.id, &batch).unwrap();
            batch.clear();
        }
    }
    batch.push(Entry {
        path: "/synthetic".into(),
        parent: "/".into(),
        name: "synthetic".into(),
        kind: EntryKind::Directory,
        logical_bytes: count * 4096,
        allocated_bytes: count * 4096,
        modified_at: Some(1),
        created_at: None,
        accessed_at: None,
        extension: String::new(),
        category: "other".into(),
        confidence: 0.0,
        evidence: vec![],
        identity,
        depth: 0,
    });
    store.insert_batch(&scan.id, &batch).unwrap();
    scan.status = "completed".into();
    scan.completed_at = Some(now());
    scan.entries = count + count.div_ceil(1000) + 1;
    store.finish_scan(&scan, 90).unwrap();
    let insertion = start.elapsed();
    let start = Instant::now();
    let dirs = engine
        .files(&FileQuery {
            kind: Some("directory".into()),
            limit: 100,
            ..Default::default()
        })
        .unwrap();
    let query = start.elapsed();
    assert_eq!(dirs.items[0].logical_bytes, count * 4096);
    let start = Instant::now();
    let categories = engine.categories().unwrap();
    let categories_us = start.elapsed().as_micros();
    let start = Instant::now();
    let insights = engine.insights().unwrap();
    println!(
        "{}",
        serde_json::json!({"experience_records": count, "categories_us": categories_us, "category_count": categories.len(), "insights_us": start.elapsed().as_micros(), "insight_count": insights.len()})
    );
    println!(
        "{}",
        serde_json::json!({"synthetic_file_records":count,"sqlite_insert_publish_ms":insertion.as_millis(),"largest_directory_query_us":query.as_micros(),"database_bytes":std::fs::metadata(engine.config.data_dir.join("index.sqlite3")).unwrap().len()})
    );
}
