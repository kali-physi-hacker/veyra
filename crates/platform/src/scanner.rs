//! Filesystem traversal. Worker threads list directories and stat their children; one
//! coordinator, the calling thread, owns the directory tree. That keeps `emit` on a single
//! thread, delivers every directory after all of its descendants with final totals, and lets
//! running totals be published as provisional entries while large directories are still open.
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::OsStr,
    fs::{self, Metadata},
    path::{Path, PathBuf},
    sync::{
        Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    time::{Duration, Instant, UNIX_EPOCH},
};
use stratum_domain::*;

/// How often the totals of still-open directories are republished.
const PROVISIONAL_INTERVAL: Duration = Duration::from_millis(400);
/// Nesting at or beyond this depth is reported and not descended into.
const MAX_DEPTH: u32 = 256;
/// Reports buffered between the workers and the coordinator. Beyond this the workers block,
/// so a slow consumer bounds memory instead of filling it with entries.
const REPORT_CAPACITY: usize = 8192;

#[derive(Clone, Default)]
pub struct ScanControl {
    pub cancelled: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
}
use std::sync::Arc;
impl ScanControl {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }
    pub fn checkpoint(&self) -> bool {
        while self.paused.load(Ordering::Relaxed) && !self.cancelled.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
        }
        !self.cancelled.load(Ordering::Relaxed)
    }
}
pub enum ScanMessage {
    /// A complete entry. A directory arrives after every one of its descendants and carries
    /// its final totals.
    Entry(Box<Entry>),
    /// A directory that is still open, with the totals gathered so far. Its complete `Entry`
    /// follows later; consumers may show or replace provisional rows but never count them.
    Provisional(Box<Entry>),
    Warning {
        path: String,
        code: String,
        message: String,
    },
    /// Children a listing skipped because of exclusions, ignore patterns, hidden names or
    /// another filesystem.
    Excluded(u64),
}

#[cfg(unix)]
pub fn identity(meta: &Metadata) -> Identity {
    use std::os::unix::fs::MetadataExt;
    Identity {
        device: meta.dev(),
        inode: meta.ino(),
        size: meta.size(),
        modified_ns: meta
            .mtime()
            .saturating_mul(1_000_000_000)
            .saturating_add(meta.mtime_nsec()),
        changed_ns: meta
            .ctime()
            .saturating_mul(1_000_000_000)
            .saturating_add(meta.ctime_nsec()),
        links: meta.nlink(),
    }
}
#[cfg(not(unix))]
pub fn identity(meta: &Metadata) -> Identity {
    Identity {
        device: 0,
        inode: 0,
        size: meta.len(),
        modified_ns: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos().min(i64::MAX as u128) as i64),
        changed_ns: 0,
        links: 0,
    }
}
#[cfg(unix)]
fn allocated(meta: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks().saturating_mul(512)
}
#[cfg(not(unix))]
fn allocated(meta: &Metadata) -> u64 {
    meta.len()
}
fn timestamp(t: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    t.ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

pub fn read_entry(path: &Path, depth: u32) -> Result<Entry> {
    let path_text = path.to_str().ok_or_else(|| {
        Error::new(
            "unsupported_path_encoding",
            "Non UTF-8 path cannot be represented in API v1",
        )
    })?;
    let meta = fs::symlink_metadata(path)?;
    let kind = if meta.file_type().is_symlink() {
        EntryKind::Symlink
    } else if meta.is_dir() {
        EntryKind::Directory
    } else if meta.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let (category, confidence, reason) = classify(path);
    Ok(Entry {
        path: path_text.into(),
        parent: path.parent().and_then(Path::to_str).unwrap_or("").into(),
        name: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path_text)
            .into(),
        logical_bytes: if kind == EntryKind::File {
            meta.len()
        } else {
            0
        },
        allocated_bytes: if kind == EntryKind::File {
            allocated(&meta)
        } else {
            0
        },
        modified_at: timestamp(meta.modified()),
        created_at: timestamp(meta.created()),
        accessed_at: timestamp(meta.accessed()),
        extension: lowercase(path.extension().and_then(OsStr::to_str).unwrap_or("")).into_owned(),
        category: category.into(),
        confidence,
        evidence: vec![Evidence::new("path_classification", reason)],
        identity: identity(&meta),
        kind,
        depth,
    })
}

/// Lowercase only when something is upper case; most names need no copy.
fn lowercase(s: &str) -> Cow<'_, str> {
    if s.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(s.to_ascii_lowercase())
    } else {
        Cow::Borrowed(s)
    }
}
fn ends_with_ci(s: &str, suffix: &str) -> bool {
    s.len() >= suffix.len()
        && s.is_char_boundary(s.len() - suffix.len())
        && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

pub fn classify(path: &Path) -> (&'static str, f32, &'static str) {
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    let has = |v: &str| components.iter().any(|c| c.eq_ignore_ascii_case(v));
    if components
        .windows(2)
        .any(|c| ends_with_ci(c[0], ".app") && c[1].eq_ignore_ascii_case("contents"))
    {
        return (
            "applications",
            0.98,
            "Inside an application bundle; association requires bundle metadata",
        );
    }
    if has("node_modules") {
        return (
            "build_artifacts",
            0.95,
            "node_modules dependency directory; may contain local edits",
        );
    }
    if has("target") {
        return (
            "build_artifacts",
            0.7,
            "Directory named target; Cargo.toml must be verified before cleanup",
        );
    }
    if has("deriveddata") || has(".gradle") || has(".m2") {
        return (
            "build_artifacts",
            0.9,
            "Known developer build/cache directory name",
        );
    }
    if has(".venv") || has("venv") {
        return (
            "developer_environments",
            0.75,
            "Virtual environment directory name; may contain user data",
        );
    }
    if has(".git") {
        return (
            "source_code",
            0.99,
            "Git metadata includes source history and must be preserved",
        );
    }
    if has(".npm") || has(".yarn") || has(".pnpm-store") || (has(".cargo") && has("registry")) {
        return ("package_caches", 0.95, "Known package cache directory");
    }
    if has("caches") || has(".cache") {
        return (
            "application_caches",
            0.8,
            "Cache directory name is a heuristic, not proof of expendability",
        );
    }
    if has("logs") {
        return ("logs", 0.8, "Log directory name");
    }
    if has("application support") || has("containers") || has("group containers") {
        return ("application_data", 0.85, "macOS application data directory");
    }
    if has("downloads") {
        return (
            "downloads",
            0.95,
            "Located under a Downloads directory; not automatically disposable",
        );
    }
    let ext = lowercase(path.extension().and_then(OsStr::to_str).unwrap_or(""));
    let category = match &*ext {
        "rs" | "c" | "cpp" | "h" | "py" | "ts" | "js" | "go" | "java" | "swift" => "source_code",
        "jpg" | "jpeg" | "png" | "gif" | "heic" | "svg" | "webp" => "images",
        "mp4" | "mov" | "mkv" | "avi" => "videos",
        "mp3" | "wav" | "flac" | "m4a" => "audio",
        "zip" | "tar" | "gz" | "7z" => "archives",
        "dmg" | "iso" => "disk_images",
        "pdf" | "docx" | "txt" | "md" | "xlsx" => "documents",
        "vmdk" | "qcow2" | "vdi" => "virtual_machines",
        "log" => "logs",
        _ => "other",
    };
    (
        category,
        if category == "other" { 0.0 } else { 0.7 },
        "Extension-based classification; content not inspected",
    )
}

/// The request's exclusions, patterns, hidden-name rule and filesystem boundary.
struct Filter {
    ignores: GlobSet,
    excluded: Vec<PathBuf>,
    include_hidden: bool,
    cross_filesystems: bool,
    device: u64,
}
impl Filter {
    fn new(request: &ScanRequest, device: u64) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        for pattern in &request.ignore_patterns {
            builder.add(Glob::new(pattern).map_err(|e| Error::invalid(e.to_string()))?);
        }
        Ok(Self {
            ignores: builder.build().map_err(|e| Error::invalid(e.to_string()))?,
            excluded: request.exclusions.iter().map(PathBuf::from).collect(),
            include_hidden: request.include_hidden,
            cross_filesystems: request.cross_filesystems,
            device,
        })
    }
    fn skips(&self, path: &Path, name: &OsStr) -> bool {
        self.excluded.iter().any(|e| path.starts_with(e))
            || self.ignores.is_match(path)
            || (!self.include_hidden && name.as_encoded_bytes().first() == Some(&b'.'))
    }
}

/// A directory waiting to be listed.
struct Job {
    path: String,
    depth: u32,
}
/// Directories are taken from the back, so the frontier of open directories stays small, the
/// way a depth-first walk keeps it small.
#[derive(Default)]
struct Queue {
    state: Mutex<(Vec<Job>, bool)>,
    ready: Condvar,
}
impl Queue {
    fn lock(&self) -> MutexGuard<'_, (Vec<Job>, bool)> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn push(&self, job: Job) {
        self.lock().0.push(job);
        self.ready.notify_one();
    }
    fn pop(&self) -> Option<Job> {
        let mut state = self.lock();
        loop {
            if state.1 {
                return None;
            }
            if let Some(job) = state.0.pop() {
                return Some(job);
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
    fn close(&self) {
        self.lock().1 = true;
        self.ready.notify_all();
    }
}

struct Warning {
    path: String,
    code: &'static str,
    message: String,
}
/// What a worker tells the coordinator. Reports from one worker arrive in order, so a
/// directory's children are always seen before its `Listed`.
enum Report {
    File(Box<Entry>),
    Dir(Box<Entry>),
    Listed {
        path: String,
        excluded: u64,
        warnings: Vec<Warning>,
    },
    Unreadable {
        path: String,
        message: String,
    },
}

fn worker(queue: &Queue, filter: &Filter, control: &ScanControl, reports: &SyncSender<Report>) {
    while let Some(job) = queue.pop() {
        if !control.checkpoint() {
            return;
        }
        let listing = match fs::read_dir(&job.path) {
            Ok(listing) => listing,
            Err(e) => {
                if reports
                    .send(Report::Unreadable {
                        path: job.path,
                        message: e.to_string(),
                    })
                    .is_err()
                {
                    return;
                }
                continue;
            }
        };
        let mut excluded = 0;
        let mut warnings = Vec::new();
        for child in listing {
            let child = match child {
                Ok(child) => child,
                Err(e) => {
                    warnings.push(Warning {
                        path: job.path.clone(),
                        code: "read_directory",
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            let path = child.path();
            if filter.skips(&path, &child.file_name()) {
                excluded += 1;
                continue;
            }
            let entry = match read_entry(&path, job.depth + 1) {
                Ok(entry) => entry,
                Err(e) => {
                    warnings.push(Warning {
                        path: path.to_string_lossy().into_owned(),
                        code: e.code,
                        message: e.message,
                    });
                    continue;
                }
            };
            if !filter.cross_filesystems && entry.identity.device != filter.device {
                excluded += 1;
                continue;
            }
            let report = if entry.kind == EntryKind::Directory {
                Report::Dir(Box::new(entry))
            } else {
                Report::File(Box::new(entry))
            };
            if reports.send(report).is_err() {
                return;
            }
        }
        if reports
            .send(Report::Listed {
                path: job.path,
                excluded,
                warnings,
            })
            .is_err()
        {
            return;
        }
    }
}

/// An open directory: its entry accumulates child totals until every child is done.
struct Node {
    entry: Entry,
    /// Subdirectories discovered and not yet complete.
    pending: usize,
    listed: bool,
    dirty: bool,
}
/// The open part of the tree. Completed directories leave it as soon as they are emitted.
struct Tree {
    root: String,
    nodes: HashMap<String, Node>,
    dirty: Vec<String>,
}
fn closed<T>() -> Result<T> {
    Err(Error::new("scan_cancelled", "Consumer closed"))
}
impl Tree {
    fn new(root: Entry) -> Self {
        let path = root.path.clone();
        let mut nodes = HashMap::new();
        nodes.insert(
            path.clone(),
            Node {
                entry: root,
                pending: 0,
                listed: false,
                dirty: false,
            },
        );
        Self {
            root: path,
            nodes,
            dirty: Vec::new(),
        }
    }
    fn add_bytes(&mut self, parent: &str, logical: u64, allocated: u64) {
        if let Some(node) = self.nodes.get_mut(parent) {
            node.entry.logical_bytes = node.entry.logical_bytes.saturating_add(logical);
            node.entry.allocated_bytes = node.entry.allocated_bytes.saturating_add(allocated);
            if !node.dirty {
                node.dirty = true;
                self.dirty.push(parent.to_owned());
            }
        }
    }
    /// Emit a finished directory and roll its totals into its parent, continuing upward while
    /// parents finish too. Returns whether the root itself completed.
    fn complete(
        &mut self,
        path: String,
        emit: &mut impl FnMut(ScanMessage) -> bool,
    ) -> Result<bool> {
        let mut current = path;
        loop {
            let Some(node) = self.nodes.remove(&current) else {
                return Ok(false);
            };
            let entry = node.entry;
            let (parent, logical, allocated) = (
                entry.parent.clone(),
                entry.logical_bytes,
                entry.allocated_bytes,
            );
            let is_root = current == self.root;
            if !emit(ScanMessage::Entry(Box::new(entry))) {
                return closed();
            }
            if is_root {
                return Ok(true);
            }
            self.add_bytes(&parent, logical, allocated);
            let Some(node) = self.nodes.get_mut(&parent) else {
                return Ok(false);
            };
            node.pending = node.pending.saturating_sub(1);
            if node.listed && node.pending == 0 {
                current = parent;
            } else {
                return Ok(false);
            }
        }
    }
    /// Republish every directory whose totals changed since the last tick.
    fn tick(&mut self, emit: &mut impl FnMut(ScanMessage) -> bool) -> Result<()> {
        for path in std::mem::take(&mut self.dirty) {
            if let Some(node) = self.nodes.get_mut(&path) {
                node.dirty = false;
                if !emit(ScanMessage::Provisional(Box::new(node.entry.clone()))) {
                    return closed();
                }
            }
        }
        Ok(())
    }
    /// Apply one report. Returns whether the root completed.
    fn apply(
        &mut self,
        report: Report,
        queue: &Queue,
        emit: &mut impl FnMut(ScanMessage) -> bool,
    ) -> Result<bool> {
        match report {
            Report::File(entry) => {
                self.add_bytes(&entry.parent, entry.logical_bytes, entry.allocated_bytes);
                if !emit(ScanMessage::Entry(entry)) {
                    return closed();
                }
            }
            Report::Dir(entry) => {
                if entry.depth >= MAX_DEPTH {
                    if !emit(ScanMessage::Warning {
                        path: entry.path,
                        code: "depth_limit".into(),
                        message: format!("Maximum traversal depth {MAX_DEPTH} reached"),
                    }) {
                        return closed();
                    }
                    return Ok(false);
                }
                if let Some(parent) = self.nodes.get_mut(&entry.parent) {
                    parent.pending += 1;
                }
                let job = Job {
                    path: entry.path.clone(),
                    depth: entry.depth,
                };
                self.nodes.insert(
                    entry.path.clone(),
                    Node {
                        entry: *entry,
                        pending: 0,
                        listed: false,
                        dirty: false,
                    },
                );
                queue.push(job);
            }
            Report::Listed {
                path,
                excluded,
                warnings,
            } => {
                for warning in warnings {
                    if !emit(ScanMessage::Warning {
                        path: warning.path,
                        code: warning.code.into(),
                        message: warning.message,
                    }) {
                        return closed();
                    }
                }
                if excluded > 0 && !emit(ScanMessage::Excluded(excluded)) {
                    return closed();
                }
                if let Some(node) = self.nodes.get_mut(&path) {
                    node.listed = true;
                    if node.pending == 0 {
                        return self.complete(path, emit);
                    }
                }
            }
            Report::Unreadable { path, message } => {
                if !emit(ScanMessage::Warning {
                    path: path.clone(),
                    code: "permission_or_io".into(),
                    message,
                }) {
                    return closed();
                }
                if let Some(node) = self.nodes.get_mut(&path) {
                    node.listed = true;
                    if node.pending == 0 {
                        return self.complete(path, emit);
                    }
                }
            }
        }
        Ok(false)
    }
}

/// Worker threads by default: enough to overlap metadata reads on one disk, never so many
/// that they contend for it.
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 8))
}

/// Traverse `root` with the default number of worker threads.
pub fn scan(
    root: &Path,
    request: &ScanRequest,
    control: &ScanControl,
    emit: impl FnMut(ScanMessage) -> bool,
) -> Result<()> {
    scan_with_threads(root, request, control, 0, emit)
}

/// Traverse `root` with `threads` workers (0 selects the default). `emit` is called on the
/// calling thread only; returning false from it stops the scan.
pub fn scan_with_threads(
    root: &Path,
    request: &ScanRequest,
    control: &ScanControl,
    threads: usize,
    mut emit: impl FnMut(ScanMessage) -> bool,
) -> Result<()> {
    let root_entry = read_entry(root, 0)?;
    if root_entry.kind != EntryKind::Directory {
        return Err(Error::invalid("Scan roots must be directories"));
    }
    // An unreadable root is an error, not a partial scan.
    fs::read_dir(root)?;
    let filter = Filter::new(request, root_entry.identity.device)?;
    let threads = if threads == 0 {
        default_threads()
    } else {
        threads.clamp(1, 64)
    };
    let mut tree = Tree::new(root_entry);
    let queue = Queue::default();
    queue.push(Job {
        path: tree.root.clone(),
        depth: 0,
    });
    let (reports, inbox) = mpsc::sync_channel(REPORT_CAPACITY);
    std::thread::scope(|scope| {
        let (queue, filter) = (&queue, &filter);
        for _ in 0..threads {
            let reports = reports.clone();
            scope.spawn(move || worker(queue, filter, control, &reports));
        }
        drop(reports);
        let result = coordinate(&mut tree, queue, &inbox, control, &mut emit);
        // Release the workers however the scan ended; cancelling also wakes paused ones.
        queue.close();
        if result.is_err() {
            control.cancel();
        }
        result
    })
}

fn coordinate(
    tree: &mut Tree,
    queue: &Queue,
    inbox: &Receiver<Report>,
    control: &ScanControl,
    emit: &mut impl FnMut(ScanMessage) -> bool,
) -> Result<()> {
    let cancelled = || {
        Err(Error::new(
            "scan_cancelled",
            "Scan cancelled; previous published index retained",
        ))
    };
    let mut last_tick = Instant::now();
    loop {
        if !control.checkpoint() {
            return cancelled();
        }
        match inbox.recv_timeout(Duration::from_millis(50)) {
            Ok(report) => {
                if tree.apply(report, queue, emit)? {
                    return Ok(());
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return if control.cancelled.load(Ordering::Relaxed) {
                    cancelled()
                } else {
                    Err(Error::new(
                        "internal_error",
                        "Scanner workers stopped before the root completed",
                    ))
                };
            }
        }
        if last_tick.elapsed() >= PROVISIONAL_INTERVAL {
            tree.tick(emit)?;
            last_tick = Instant::now();
        }
    }
}
