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
    collections::HashMap,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
};
pub use stratum_domain as domain;
use stratum_domain::*;
use stratum_index::Store;
use stratum_platform::scanner::{self, ScanControl, ScanMessage};
use tokio::sync::broadcast;

pub struct Engine {
    pub config: Config,
    pub(crate) store: Store,
    events: broadcast::Sender<OperationEvent>,
    controls: Mutex<HashMap<String, ScanControl>>,
    pub(crate) watcher_running: std::sync::atomic::AtomicBool,
}
impl Engine {
    pub fn open(mut config: Config) -> Result<Arc<Self>> {
        if config.batch_size == 0
            || config.batch_size > 10000
            || config.scan_queue_capacity == 0
            || config.scan_queue_capacity > 10000
        {
            return Err(Error::invalid(
                "Batch and queue capacities must be 1..10000",
            ));
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
        let (tx, rx) = mpsc::sync_channel(self.config.scan_queue_capacity);
        let req = request.clone();
        let ctl = control.clone();
        let worker = std::thread::spawn(move || {
            scanner::scan(&root, &req, &ctl, |msg| tx.send(msg).is_ok())
        });
        let mut batch = Vec::with_capacity(self.config.batch_size);
        let mut indexed_bytes = 0u64;
        let mut persistence_error = None;
        for message in rx {
            let result: Result<()> = match message {
                ScanMessage::Entry(entry) => {
                    record.entries += 1;
                    if entry.depth == 0 {
                        record.logical_bytes = entry.logical_bytes;
                        record.allocated_bytes = entry.allocated_bytes;
                    }
                    if entry.kind == EntryKind::File {
                        indexed_bytes = indexed_bytes.saturating_add(entry.logical_bytes);
                    }
                    if self.events.receiver_count() > 0 {
                        self.emit(OperationEvent::PathIndexed {
                            scan_id: record.id.clone(),
                            path: entry.path.clone(),
                        });
                    }
                    if entry.kind == EntryKind::Directory && entry.depth <= 1 {
                        self.emit(OperationEvent::DirectoryCompleted {
                            scan_id: record.id.clone(),
                            path: entry.path.clone(),
                            bytes: entry.logical_bytes,
                        });
                    }
                    batch.push(*entry);
                    if batch.len() >= self.config.batch_size {
                        let r = self.store.insert_batch(&record.id, &batch);
                        batch.clear();
                        self.emit(OperationEvent::ScanProgress {
                            scan_id: record.id.clone(),
                            entries: record.entries,
                            bytes: indexed_bytes,
                        });
                        r
                    } else {
                        Ok(())
                    }
                }
                ScanMessage::Warning {
                    path,
                    code,
                    message,
                } => {
                    record.warnings += 1;
                    self.emit(OperationEvent::ScanWarning {
                        scan_id: record.id.clone(),
                        path: path.clone(),
                        code: code.clone(),
                        message: message.clone(),
                    });
                    self.store.warning(&record.id, &path, &code, &message)
                }
                ScanMessage::Excluded => {
                    record.excluded += 1;
                    Ok(())
                }
            };
            if let Err(error) = result {
                persistence_error = Some(error);
                control.cancel();
                break;
            }
        }
        let traversal = worker
            .join()
            .unwrap_or_else(|_| Err(Error::new("internal_error", "Scanner worker panicked")));
        if persistence_error.is_none()
            && let Err(e) = self.store.insert_batch(&record.id, &batch)
        {
            persistence_error = Some(e);
        }
        let failure = persistence_error.or_else(|| traversal.err());
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
        self.store
            .finish_scan(&record, self.config.history_retention_days)?;
        self.controls
            .lock()
            .map_err(|_| Error::new("internal_error", "Control lock poisoned"))?
            .remove(&record.id);
        self.emit(OperationEvent::ScanCompleted {
            scan: record.clone(),
        });
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
        Ok(StorageExplanation {
            resources:stratum_platform::system::summary(),
            scans:self.scans()?,categories:self.categories()?,
            largest_directories:self.files(&FileQuery {kind:Some("directory".into()),limit:20,..Default::default()})?.items,
            insights:self.insights()?,history:self.history(None,now()-7*86400)?,
            interpretation:"Sizes describe indexed paths; hard links and APFS clones can share physical blocks. Missing permissions, exclusions and unscanned roots reduce coverage. History compares observed scans, not continuous change attribution.".into(),
            coverage:self.coverage()?,
        })
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
