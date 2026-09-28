//! Time the traversal alone: `cargo run --release -p stratum-platform --example walk -- DIR`.
//! `STRATUM_SCAN_THREADS=n` overrides the worker count (0 is the default choice).
use std::{path::PathBuf, time::Instant};
use stratum_platform::scanner::{ScanControl, ScanMessage, scan_with_threads};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("usage: walk DIR")?);
    let threads = std::env::var("STRATUM_SCAN_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let (mut entries, mut directories, mut provisional, mut warnings, mut excluded, mut bytes) =
        (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let start = Instant::now();
    scan_with_threads(
        &root,
        &stratum_domain::ScanRequest::default(),
        &ScanControl::default(),
        threads,
        |message| {
            match message {
                ScanMessage::Entry(e) => {
                    entries += 1;
                    if e.depth == 0 {
                        bytes = e.logical_bytes;
                    }
                    if e.kind == stratum_domain::EntryKind::Directory {
                        directories += 1;
                    }
                }
                ScanMessage::Provisional(_) => provisional += 1,
                ScanMessage::Warning { .. } => warnings += 1,
                ScanMessage::Excluded(n) => excluded += n,
            }
            true
        },
    )?;
    println!(
        "{}",
        serde_json::json!({"threads":threads,"entries":entries,"directories":directories,"provisional":provisional,"warnings":warnings,"excluded":excluded,"logical_bytes":bytes,"elapsed_ms":start.elapsed().as_millis()})
    );
    Ok(())
}
