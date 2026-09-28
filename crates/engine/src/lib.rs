//! Public application API shared by CLI, desktop, HTTP and future MCP adapters.
mod analysis_rules;
pub mod applications;
pub mod cleanup;
pub mod daemon;
pub mod duplicates;
pub mod incremental;
pub mod intelligence;
pub mod jobs;
use std::{
    any::Any,
    collections::HashMap,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
pub use stratum_domain as domain;
use stratum_domain::*;
use stratum_index::Store;
use stratum_platform::scanner::{self, ScanControl, ScanMessage};
use tokio::sync::broadcast;

/// How long a scan batch may wait before it is written, so pages reading the visible
/// generation see progress at a steady pace even while the batch is far from full.
const FLUSH_INTERVAL: Duration = Duration::from_millis(1000);
/// Per-path events are samples, not a log of every entry.
const PATH_EVENT_INTERVAL: Duration = Duration::from_millis(100);

type Cached = (u64, Arc<dyn Any + Send + Sync>);

pub struct Engine {
    pub config: Config,
    pub(crate) store: Store,
    events: broadcast::Sender<OperationEvent>,
    controls: Mutex<HashMap<String, ScanControl>>,
    /// Derived views keyed by the index version they were computed from.
    cache: Mutex<HashMap<&'static str, Cached>>,
    pub(crate) watcher_running: std::sync::atomic::AtomicBool,
}
/// Restores full durability when a scan ends, however it ends.
struct Bulk<'a>(&'a Store);
impl Drop for Bulk<'_> {
    fn drop(&mut self) {
        if let Err(e) = self.0.set_bulk(false) {
            tracing::error!(error=%e,"Failed to leave bulk mode");
        }
    }
}
impl Engine {
    pub fn open(mut config: Config) -> Result<Arc<Self>> {
        if config.batch_size == 0
            || config.batch_size > 200_000
            || config.scan_queue_capacity == 0
            || config.scan_queue_capacity > 10000
        {
            return Err(Error::invalid(
                "batch_size must be 1..200000 and scan_queue_capacity 1..10000",
            ));
        }
        if config.scan_threads > 64 {
            return Err(Error::invalid("scan_threads must be 0..64"));
        }
        if config.monitoring_interval_seconds < 1 || config.reconciliation_seconds < 5 {
            return Err(Error::invalid(
                "Monitoring interval >=1s; reconciliation interval >=5s",
            ));
        }
        if config.data_dir.exists()
            && !config.data_dir.join(".stratum-state").is_file()
            && fs::read_dir(&config.data_dir)?.next().is_some()
        {
            return Err(Error::new(
                "protected_path",
                "Data directory must be empty or contain a Stratum state marker; choose a dedicated directory",
            ));
        }
        fs::create_dir_all(&config.data_dir)?;
        config.data_dir = fs::canonicalize(&config.data_dir)?;
        if config.data_dir.parent().is_none()
            || std::env::var_os("HOME").is_some_and(|p| Path::new(&p) == config.data_dir)
        {
            return Err(Error::new(
                "protected_path",
                "Use a dedicated data directory, never a filesystem or home root",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&config.data_dir, fs::Permissions::from_mode(0o700))?;
        }
        if !config.data_dir.join(".stratum-state").exists() {
            let _marker = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(config.data_dir.join(".stratum-state"))?;
        }
        let store = Store::open(&config.data_dir.join("index.sqlite3"))?;
        let (events, _) = broadcast::channel(512);
        let engine = Arc::new(Self {
            config,
            store,
            events,
            controls: Mutex::new(HashMap::new()),
            cache: Mutex::new(HashMap::new()),
            watcher_running: std::sync::atomic::AtomicBool::new(false),
        });
        if let Ok(_guard) = engine.writer_lock() {
            engine.store.recover_scans()?;
            let serialized = serde_json::to_string(&engine.config)?;
            let fingerprint = blake3::hash(serialized.as_bytes()).to_hex().to_string();
            let prior = engine.store.get::<String>("configuration", "fingerprint");
            match prior {
                Ok(prior) if prior == fingerprint => {}
                Ok(_) => {
                    engine.store.audit(
                        "configuration_changed",
                        "effective_configuration",
                        &serialized,
                    )?;
                    engine
                        .store
                        .put("configuration", "fingerprint", &fingerprint)?;
                }
                Err(e) if e.code == "not_found" => {
                    engine.store.audit(
                        "configuration_initialized",
                        "effective_configuration",
                        &serialized,
                    )?;
                    engine
                        .store
                        .put("configuration", "fingerprint", &fingerprint)?;
                }
                Err(e) => return Err(e),
            }
        }
        engine.recover_jobs()?;
        Ok(engine)
    }
    pub fn subscribe(&self) -> broadcast::Receiver<OperationEvent> {
        self.events.subscribe()
    }
    /// Read each root's visible generation: a first scan while it runs, otherwise the
    /// published index. Rescans stay invisible until they publish. Interfaces that show
    /// progress as it happens opt in; the API and CLI keep the published view.
    pub fn set_live_view(&self, on: bool) {
        self.store.set_live_view(on);
    }
    pub fn live_view(&self) -> bool {
        self.store.live_view()
    }
    /// A derived view, recomputed only after the index changes.
    pub(crate) fn cached<T: Clone + Send + Sync + 'static>(
        &self,
        key: &'static str,
        compute: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let version = self.store.version();
        if let Ok(cache) = self.cache.lock()
            && let Some((cached_version, value)) = cache.get(key)
            && *cached_version == version
            && let Some(value) = value.downcast_ref::<T>()
        {
            return Ok(value.clone());
        }
        let value = compute()?;
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(key, (version, Arc::new(value.clone())));
        }
        Ok(value)
    }
    pub fn watcher_running(&self) -> bool {
        self.watcher_running
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    pub(crate) fn emit(&self, event: OperationEvent) {
        let _ = self.events.send(event);
    }
    pub(crate) fn writer_lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.config.data_dir.join("writer.lock"))?;
        file.try_lock()
            .map_err(|_| Error::new("busy", "Another scan or cleanup operation is active"))?;
        Ok(file)
    }
    pub fn scan(self: &Arc<Self>, mut request: ScanRequest) -> Result<Vec<ScanRecord>> {
        let _guard = self.writer_lock()?;
        // A scan another process abandoned must not stay readable as a running generation.
        self.store.recover_scans()?;
        if request.roots.is_empty() {
            request.roots = self.config.scan_roots.clone();
        }
        if request.roots.is_empty() {
            return Err(Error::invalid(
                "At least one explicit scan root is required",
            ));
        }
        request.exclusions.extend(self.config.exclusions.clone());
        request
            .exclusions
            .push(self.config.data_dir.to_string_lossy().into());
        let mut roots = Vec::new();
        for root in &request.roots {
            let path = fs::canonicalize(root)?;
            if !path.is_dir() {
                return Err(Error::invalid("Scan roots must be directories"));
            }
            let text = path
                .to_str()
                .ok_or_else(|| Error::invalid("Root must be UTF-8"))?
                .to_owned();
            if roots
                .iter()
                .any(|r: &PathBuf| path.starts_with(r) || r.starts_with(&path))
            {
                return Err(Error::invalid("Requested scan roots overlap"));
            }
            for indexed in self.store.roots()? {
                if indexed != text
                    && (path.starts_with(&indexed) || Path::new(&indexed).starts_with(&path))
                {
                    return Err(Error::new(
                        "overlapping_root",
                        format!("Root overlaps indexed root {indexed}; rescan that root instead"),
                    ));
                }
            }
            roots.push(path);
        }
        for exclusion in &mut request.exclusions {
            let p = Path::new(exclusion);
            if !p.is_absolute() {
                return Err(Error::invalid("Exclusions must be absolute paths"));
            }
            if p.exists() {
                *exclusion = fs::canonicalize(p)?.to_string_lossy().into();
            }
        }
        request.exclusions.sort();
        request.exclusions.dedup();
        let mut output = Vec::new();
        for root in roots {
            output.push(self.scan_root(root, &request)?);
        }
        Ok(output)
    }
    /// A UI-friendly scan that preserves the published policy of an existing root.
    pub fn scan_location(self: &Arc<Self>, root: &str) -> Result<Vec<ScanRecord>> {
        let canonical = fs::canonicalize(root)?;
        let root = canonical
            .to_str()
            .ok_or_else(|| Error::invalid("Root must be UTF-8"))?
            .to_string();
        let mut request = if self.roots()?.contains(&root) {
            self.scan_policy(&root)?
        } else {
            ScanRequest::default()
        };
        request.roots = vec![root];
        self.scan(request)
    }
    fn scan_root(self: &Arc<Self>, root: PathBuf, request: &ScanRequest) -> Result<ScanRecord> {
        let mut record = ScanRecord {
            id: id(),
            root: root.to_string_lossy().into(),
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
        let control = ScanControl::default();
        self.controls
            .lock()
            .map_err(|_| Error::new("internal_error", "Control lock poisoned"))?
            .insert(record.id.clone(), control.clone());
        self.store.begin_scan(&record)?;
        self.store.put("scan_policy", &record.id, request)?;
        self.emit(OperationEvent::ScanStarted {
            scan_id: record.id.clone(),
            root: record.root.clone(),
        });
        let bulk = Bulk(&self.store);
        self.store.set_bulk(true)?;
        let (tx, rx) = mpsc::sync_channel(self.config.scan_queue_capacity);
        let req = request.clone();
        let ctl = control.clone();
        let threads = self.config.scan_threads;
        let worker = std::thread::spawn(move || {
            scanner::scan_with_threads(&root, &req, &ctl, threads, |msg| tx.send(msg).is_ok())
        });
        let mut batch = Vec::with_capacity(self.config.batch_size);
        let mut indexed_bytes = 0u64;
        let mut persistence_error = None;
        let mut last_flush = Instant::now();
        let mut last_path_event = Instant::now() - PATH_EVENT_INTERVAL;
        let streaming = Instant::now();
        loop {
            let message = match rx.recv_timeout(FLUSH_INTERVAL) {
                Ok(message) => Some(message),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let mut result: Result<()> = Ok(());
            match message {
                Some(ScanMessage::Entry(entry)) => {
                    record.entries += 1;
                    if entry.depth == 0 {
                        record.logical_bytes = entry.logical_bytes;
                        record.allocated_bytes = entry.allocated_bytes;
                    }
                    if entry.kind == EntryKind::File {
                        indexed_bytes = indexed_bytes.saturating_add(entry.logical_bytes);
                    }
                    if entry.kind == EntryKind::Directory && entry.depth <= 1 {
                        self.emit(OperationEvent::DirectoryCompleted {
                            scan_id: record.id.clone(),
                            path: entry.path.clone(),
                            bytes: entry.logical_bytes,
                        });
                    } else if last_path_event.elapsed() >= PATH_EVENT_INTERVAL
                        && self.events.receiver_count() > 0
                    {
                        last_path_event = Instant::now();
                        self.emit(OperationEvent::PathIndexed {
                            scan_id: record.id.clone(),
                            path: entry.path.clone(),
                        });
                    }
                    batch.push(*entry);
                }
                // Running totals of open directories; replaced by their complete rows later.
                Some(ScanMessage::Provisional(entry)) => batch.push(*entry),
                Some(ScanMessage::Warning {
                    path,
                    code,
                    message,
                }) => {
                    record.warnings += 1;
                    self.emit(OperationEvent::ScanWarning {
                        scan_id: record.id.clone(),
                        path: path.clone(),
                        code: code.clone(),
                        message: message.clone(),
                    });
                    result = self.store.warning(&record.id, &path, &code, &message);
                }
                Some(ScanMessage::Excluded(count)) => record.excluded += count,
                None => {}
            }
            if result.is_ok()
                && !batch.is_empty()
                && (batch.len() >= self.config.batch_size || last_flush.elapsed() >= FLUSH_INTERVAL)
            {
                result = self.store.insert_batch(&record.id, &batch);
                batch.clear();
                last_flush = Instant::now();
                self.emit(OperationEvent::ScanProgress {
                    scan_id: record.id.clone(),
                    entries: record.entries,
                    bytes: indexed_bytes,
                });
            }
            if let Err(error) = result {
                persistence_error = Some(error);
                control.cancel();
                break;
            }
        }
        // Closing the receiver frees a scanner blocked on a full queue, so the join cannot hang.
        drop(rx);
        let traversal = worker
            .join()
            .unwrap_or_else(|_| Err(Error::new("internal_error", "Scanner worker panicked")));
        if persistence_error.is_none()
            && let Err(e) = self.store.insert_batch(&record.id, &batch)
        {
            persistence_error = Some(e);
        }
        let failure = persistence_error.or_else(|| traversal.err());
        let streamed_ms = streaming.elapsed().as_millis();
        record.completed_at = Some(now());
        record.status = match &failure {
            Some(e) if e.code == "scan_cancelled" => "cancelled",
            Some(_) => "failed",
            None if record.warnings > 0 => "partial",
            None => "completed",
        }
        .into();
        record.freshness = if failure.is_none() && record.warnings == 0 {
            "probably_fresh"
        } else {
            "unknown"
        }
        .into();
        let publishing = Instant::now();
        self.store
            .finish_scan(&record, self.config.history_retention_days)?;
        let published_ms = publishing.elapsed().as_millis();
        self.controls
            .lock()
            .map_err(|_| Error::new("internal_error", "Control lock poisoned"))?
            .remove(&record.id);
        // Announce before the checkpoint that follows bulk mode; pages can refresh at once.
        self.emit(OperationEvent::ScanCompleted {
            scan: record.clone(),
        });
        drop(bulk);
        tracing::info!(
            scan = %record.id,
            entries = record.entries,
            streamed_ms,
            published_ms,
            checkpoint_ms = publishing.elapsed().as_millis() - published_ms,
            "scan phases"
        );
        if let Some(e) = failure {
            return Err(e);
        }
        Ok(record)
    }
    pub fn control_scan(&self, id: &str, action: &str) -> Result<()> {
        let controls = self
            .controls
            .lock()
            .map_err(|_| Error::new("internal_error", "Control lock poisoned"))?;
        let c = controls
            .get(id)
            .ok_or_else(|| Error::new("not_found", "Scan is not active in this service"))?;
        match action {
            "cancel" => c.cancel(),
            "pause" => c.pause(true),
            "resume" => c.pause(false),
            _ => return Err(Error::invalid("Expected cancel, pause or resume")),
        };
        Ok(())
    }
    pub fn cancel_all(&self) {
        if let Ok(controls) = self.controls.lock() {
            for c in controls.values() {
                c.cancel();
            }
        }
    }
    pub fn files(&self, query: &FileQuery) -> Result<Page<Entry>> {
        self.store.files(query)
    }
    pub fn directory_breakdown(&self, path: &str, limit: u32) -> Result<DirectoryBreakdown> {
        self.store.directory_breakdown(path, limit)
    }
    pub fn inspect_entry(&self, path: &str) -> Result<Entry> {
        self.store.entry(path)
    }
    pub fn scan_policy(&self, root: &str) -> Result<ScanRequest> {
        self.store.scan_policy(root)
    }
    pub fn roots(&self) -> Result<Vec<String>> {
        self.store.roots()
    }
    pub fn warnings(&self, id: &str) -> Result<Vec<Evidence>> {
        self.store.warnings(id)
    }
    pub fn audit(&self, limit: u32, offset: u64) -> Result<Vec<AuditRecord>> {
        self.store.audit_records(limit, offset)
    }
    pub fn system_history(&self) -> Result<Vec<serde_json::Value>> {
        self.store.system_history()
    }
    pub fn cleanup_operations(&self) -> Result<Vec<CleanupOperation>> {
        self.store.documents("operation")
    }
    pub fn scans(&self) -> Result<Vec<ScanRecord>> {
        Ok(self.adjust_freshness(self.store.scans()?))
    }
    pub fn coverage(&self) -> Result<Vec<ScanRecord>> {
        Ok(self.adjust_freshness(self.store.published_scans()?))
    }
    fn adjust_freshness(&self, mut scans: Vec<ScanRecord>) -> Vec<ScanRecord> {
        for scan in &mut scans {
            if scan.freshness == "probably_fresh"
                && scan
                    .completed_at
                    .is_some_and(|t| now() - t > self.config.reconciliation_seconds as i64)
            {
                scan.freshness = "stale".into();
            }
        }
        scans
    }
    pub fn categories(&self) -> Result<Vec<CategoryTotal>> {
        self.store.categories()
    }
    pub fn history(&self, path: Option<&str>, since: i64) -> Result<Vec<HistoryPoint>> {
        self.store.history(path, since)
    }
    pub fn system(&self) -> SystemSnapshot {
        stratum_platform::system::snapshot()
    }
    pub fn explain_storage(&self) -> Result<StorageExplanation> {
        let mut explanation = self.cached("explain_storage", || {
            Ok(StorageExplanation {
                resources: stratum_platform::system::summary(),
                scans: self.scans()?,
                categories: self.categories()?,
                largest_directories: self
                    .files(&FileQuery {
                        kind: Some("directory".into()),
                        limit: 20,
                        ..Default::default()
                    })?
                    .items,
                insights: self.insights()?,
                history: self.history(None, now() - 7 * 86400)?,
                interpretation: "Sizes describe indexed paths; hard links and APFS clones can share physical blocks. Missing permissions, exclusions and unscanned roots reduce coverage. History compares observed scans, not continuous change attribution.".into(),
                coverage: self.coverage()?,
            })
        })?;
        // Capacity and load are live observations, never served from the cache.
        explanation.resources = stratum_platform::system::summary();
        Ok(explanation)
    }
}

pub fn load_config(path: Option<&Path>, data_dir: Option<PathBuf>) -> Result<Config> {
    let mut config = if let Some(path) = path {
        toml::from_str(&fs::read_to_string(path)?)
            .map_err(|e| Error::invalid(format!("Invalid configuration: {e}")))?
    } else {
        Config::default()
    };
    if let Some(dir) = data_dir.or_else(|| std::env::var_os("STRATUM_DATA_DIR").map(PathBuf::from))
    {
        config.data_dir = dir;
    }
    Ok(config)
}
