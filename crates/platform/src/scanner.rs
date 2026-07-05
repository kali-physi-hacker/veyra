use globset::{Glob, GlobSetBuilder};
use std::{
    fs::{self, Metadata},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};
use stratum_domain::*;

#[derive(Clone, Default)]
pub struct ScanControl {
    pub cancelled: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
}
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
    Entry(Box<Entry>),
    Warning {
        path: String,
        code: String,
        message: String,
    },
    Excluded,
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
        extension: path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase(),
        category: category.into(),
        confidence,
        evidence: vec![Evidence::new("path_classification", reason)],
        identity: identity(&meta),
        kind,
        depth,
    })
}

pub fn classify(path: &Path) -> (&'static str, f32, &'static str) {
    let components: Vec<_> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    let has = |v: &str| components.iter().any(|c| c == v);
    if components
        .windows(2)
        .any(|c| c[0].ends_with(".app") && c[1] == "contents")
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
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let category = match ext.as_str() {
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

/// Depth-first streaming traversal: memory and descriptors bounded by depth (max 256).
/// One producer plus a bounded channel allows the persistence consumer to apply backpressure.
pub fn scan(
    root: &Path,
    request: &ScanRequest,
    control: &ScanControl,
    mut emit: impl FnMut(ScanMessage) -> bool,
) -> Result<()> {
    let mut builder = GlobSetBuilder::new();
    for pattern in &request.ignore_patterns {
        builder.add(Glob::new(pattern).map_err(|e| Error::invalid(e.to_string()))?);
    }
    let ignores = builder.build().map_err(|e| Error::invalid(e.to_string()))?;
    let excluded: Vec<_> = request.exclusions.iter().map(PathBuf::from).collect();
    let root_entry = read_entry(root, 0)?;
    if root_entry.kind != EntryKind::Directory {
        return Err(Error::invalid("Scan roots must be directories"));
    }
    let device = root_entry.identity.device;
    struct Frame {
        entry: Entry,
        children: fs::ReadDir,
    }
    let mut stack = vec![Frame {
        entry: root_entry,
        children: fs::read_dir(root)?,
    }];
    while !stack.is_empty() {
        if !control.checkpoint() {
            return Err(Error::new(
                "scan_cancelled",
                "Scan cancelled; previous published index retained",
            ));
        }
        let next = stack.last_mut().expect("nonempty").children.next();
        match next {
            None => {
                let frame = stack.pop().expect("nonempty");
                if let Some(parent) = stack.last_mut() {
                    parent.entry.logical_bytes = parent
                        .entry
                        .logical_bytes
                        .saturating_add(frame.entry.logical_bytes);
                    parent.entry.allocated_bytes = parent
                        .entry
                        .allocated_bytes
                        .saturating_add(frame.entry.allocated_bytes);
                }
                if !emit(ScanMessage::Entry(Box::new(frame.entry))) {
                    return Err(Error::new("scan_cancelled", "Consumer closed"));
                }
            }
            Some(Err(e)) => {
                let p = stack.last().expect("nonempty").entry.path.clone();
                if !emit(ScanMessage::Warning {
                    path: p,
                    code: "read_directory".into(),
                    message: e.to_string(),
                }) {
                    break;
                }
            }
            Some(Ok(child)) => {
                let path = child.path();
                if excluded.iter().any(|e| path.starts_with(e))
                    || ignores.is_match(&path)
                    || (!request.include_hidden
                        && child.file_name().to_string_lossy().starts_with('.'))
                {
                    if !emit(ScanMessage::Excluded) {
                        break;
                    }
                    continue;
                }
                let entry = match read_entry(&path, stack.len() as u32) {
                    Ok(e) => e,
                    Err(e) => {
                        if !emit(ScanMessage::Warning {
                            path: path.to_string_lossy().into(),
                            code: e.code.into(),
                            message: e.message,
                        }) {
                            break;
                        }
                        continue;
                    }
                };
                if !request.cross_filesystems && entry.identity.device != device {
                    if !emit(ScanMessage::Excluded) {
                        break;
                    }
                    continue;
                }
                if entry.kind == EntryKind::Directory {
                    if stack.len() >= 256 {
                        if !emit(ScanMessage::Warning {
                            path: entry.path,
                            code: "depth_limit".into(),
                            message: "Maximum traversal depth 256 reached".into(),
                        }) {
                            break;
                        }
                        continue;
                    }
                    match fs::read_dir(&path) {
                        Ok(children) => stack.push(Frame { entry, children }),
                        Err(e) => {
                            if !emit(ScanMessage::Warning {
                                path: entry.path.clone(),
                                code: "permission_or_io".into(),
                                message: e.to_string(),
                            }) || !emit(ScanMessage::Entry(Box::new(entry)))
                            {
                                break;
                            }
                        }
                    }
                } else {
                    let parent = stack.last_mut().expect("nonempty");
                    parent.entry.logical_bytes = parent
                        .entry
                        .logical_bytes
                        .saturating_add(entry.logical_bytes);
                    parent.entry.allocated_bytes = parent
                        .entry
                        .allocated_bytes
                        .saturating_add(entry.allocated_bytes);
                    if !emit(ScanMessage::Entry(Box::new(entry))) {
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
