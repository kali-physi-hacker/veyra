//! Native filesystem events invalidate coverage; debounced reconciliations publish new generations.
use crate::Engine;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use stratum_domain::*;

pub fn run(engine: Arc<Engine>, stop: Arc<AtomicBool>) -> Result<()> {
    let lease = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(engine.config.data_dir.join("daemon.lock"))?;
    lease
        .try_lock()
        .map_err(|_| Error::new("busy", "A daemon already watches this state directory"))?;
    struct WatchGuard(Arc<Engine>);
    impl Drop for WatchGuard {
        fn drop(&mut self) {
            self.0.watcher_running.store(false, Ordering::Relaxed);
        }
    }
    let _watch_guard = WatchGuard(engine.clone());
    let (sender, receiver) = mpsc::sync_channel(1024);
    let overflow = Arc::new(AtomicBool::new(false));
    let callback_overflow = overflow.clone();
    let mut watcher: RecommendedWatcher =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if sender.try_send(event).is_err() {
                callback_overflow.store(true, Ordering::Relaxed);
            }
        })
        .map_err(|e| Error::new("watch_error", e.to_string()))?;
    let roots = if engine.config.scan_roots.is_empty() {
        engine.store.roots()?
    } else {
        engine
            .config
            .scan_roots
            .iter()
            .map(|p| std::fs::canonicalize(p).map(|p| p.to_string_lossy().into_owned()))
            .collect::<std::io::Result<Vec<_>>>()?
    };
    if roots.is_empty() {
        return Err(Error::invalid(
            "Daemon needs configured or previously indexed roots",
        ));
    }
    for root in &roots {
        watcher
            .watch(std::path::Path::new(root), RecursiveMode::Recursive)
            .map_err(|e| Error::new("watch_error", format!("{root}: {e}")))?;
    }
    let indexed = engine.store.roots()?;
    for root in &roots {
        if !indexed.contains(root) {
            engine.scan(ScanRequest {
                roots: vec![root.clone()],
                ..Default::default()
            })?;
        }
    }
    engine.watcher_running.store(true, Ordering::Relaxed);
    let mut pending = HashSet::new();
    let mut changed_paths = HashSet::new();
    let mut last_event = Instant::now();
    let mut last_reconcile = Instant::now();
    let mut last_sample =
        Instant::now() - Duration::from_secs(engine.config.monitoring_interval_seconds);
    engine.store.audit(
        "daemon_started",
        "daemon",
        "Native watch, incremental leaf updates and periodic full reconciliation",
    )?;
    while !stop.load(Ordering::Relaxed) {
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(event)) => {
                if matches!(event.kind, notify::EventKind::Access(_)) {
                    continue;
                }
                for root in &roots {
                    if event
                        .paths
                        .iter()
                        .any(|p| p.starts_with(root) && !p.starts_with(&engine.config.data_dir))
                    {
                        engine.store.mark_stale(root)?;
                        for path in &event.paths {
                            if path.starts_with(root) && !path.starts_with(&engine.config.data_dir)
                            {
                                if changed_paths.len() < 1024 {
                                    changed_paths.insert(path.to_string_lossy().into_owned());
                                } else {
                                    overflow.store(true, Ordering::Relaxed);
                                }
                            }
                        }
                        engine.emit(OperationEvent::IndexChanged {
                            root: root.clone(),
                            freshness: "stale".into(),
                        });
                        last_event = Instant::now();
                    }
                }
            }
            Ok(Err(e)) => {
                engine
                    .store
                    .audit("watch_error", "daemon", &e.to_string())?;
                overflow.store(true, Ordering::Relaxed);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Error::new("watch_error", "Filesystem watcher disconnected"));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if !changed_paths.is_empty() && last_event.elapsed() >= Duration::from_secs(2) {
            match engine.reconcile_paths(ReconcileRequest {
                paths: changed_paths.iter().cloned().collect(),
            }) {
                Ok(report) => {
                    pending.extend(report.roots_requiring_scan);
                    changed_paths.clear();
                }
                Err(e) if e.code == "busy" => {}
                Err(e) => {
                    engine
                        .store
                        .audit("incremental_failed", "daemon", &e.to_string())?;
                    pending.extend(roots.iter().cloned());
                    changed_paths.clear();
                }
            }
        }
        if overflow.swap(false, Ordering::Relaxed)
            || last_reconcile.elapsed().as_secs() >= engine.config.reconciliation_seconds
        {
            for root in &roots {
                engine.store.mark_stale(root)?;
                pending.insert(root.clone());
            }
            last_reconcile = Instant::now();
        }
        if !pending.is_empty() && last_event.elapsed() >= Duration::from_secs(2) {
            for root in pending.clone() {
                let mut request = engine.scan_policy(&root)?;
                request.roots = vec![root.clone()];
                match engine.scan(request) {
                    Ok(_) => {
                        pending.remove(&root);
                    }
                    Err(e) if e.code == "busy" => {}
                    Err(e) => {
                        engine
                            .store
                            .audit("reconciliation_failed", "daemon", &e.to_string())?;
                        pending.remove(&root);
                    }
                }
            }
        }
        if last_sample.elapsed().as_secs() >= engine.config.monitoring_interval_seconds {
            let snapshot = engine.system();
            engine
                .store
                .record_system(&snapshot, engine.config.history_retention_days)?;
            last_sample = Instant::now();
        }
    }
    engine.cancel_all();
    engine
        .store
        .audit("daemon_stopped", "daemon", "Clean shutdown")?;
    Ok(())
}
