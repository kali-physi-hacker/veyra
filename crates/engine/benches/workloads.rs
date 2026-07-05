use std::{fs, sync::atomic::AtomicBool, time::Instant};
use stratum_engine::{Engine, domain::*};
fn main() {
    let count = std::env::var("STRATUM_BENCH_FILES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(10000);
    let fixture = tempfile::tempdir().expect("fixture");
    let root = fixture.path().join("data");
    fs::create_dir(&root).unwrap();
    for i in 0..count {
        let dir = root.join(format!("d{:05}", i / 1000));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("f{i:08}.bin")),
            format!("payload-{:08}", i / 2),
        )
        .unwrap();
    }
    let engine = Engine::open(Config {
        data_dir: fixture.path().join("state"),
        ..Default::default()
    })
    .unwrap();
    let start = Instant::now();
    stratum_platform::scanner::scan(
        &root,
        &ScanRequest::default(),
        &stratum_platform::scanner::ScanControl::default(),
        |_| true,
    )
    .unwrap();
    let traversal = start.elapsed();
    let start = Instant::now();
    engine
        .scan(ScanRequest {
            roots: vec![root.to_string_lossy().into()],
            ..Default::default()
        })
        .unwrap();
    let scan = start.elapsed();
    let start = Instant::now();
    engine
        .files(&FileQuery {
            kind: Some("directory".into()),
            ..Default::default()
        })
        .unwrap();
    let query = start.elapsed();
    let start = Instant::now();
    let duplicates = engine.discover_duplicates(&AtomicBool::new(false)).unwrap();
    let duplicate_time = start.elapsed();
    fs::write(root.join("changed.bin"), "incremental").unwrap();
    let changed = fs::canonicalize(root.join("changed.bin")).unwrap();
    let start = Instant::now();
    engine
        .reconcile_paths(ReconcileRequest {
            paths: vec![changed.to_string_lossy().into()],
        })
        .unwrap();
    let incremental = start.elapsed();
    let start = Instant::now();
    engine
        .scan(ScanRequest {
            roots: vec![root.to_string_lossy().into()],
            ..Default::default()
        })
        .unwrap();
    println!(
        "{}",
        serde_json::json!({"files":count,"traversal_ms":traversal.as_millis(),"scan_and_sqlite_ms":scan.as_millis(),"largest_dirs_us":query.as_micros(),"duplicate_pipeline_ms":duplicate_time.as_millis(),"duplicate_groups":duplicates.group_count,"incremental_us":incremental.as_micros(),"reconciliation_ms":start.elapsed().as_millis()})
    );
}
