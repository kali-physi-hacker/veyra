//! Versioned, transport-independent Stratum contracts. Sizes are bytes; times are Unix seconds.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use utoipa::ToSchema;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
#[error("{code}: {message}")]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}
impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }
    pub fn io(e: std::io::Error) -> Self {
        let code = match e.kind() {
            std::io::ErrorKind::PermissionDenied => "permission_denied",
            std::io::ErrorKind::NotFound => "path_not_found",
            std::io::ErrorKind::AlreadyExists => "conflict",
            _ => "io_error",
        };
        Self::new(code, e.to_string())
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::io(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::new("serialization_error", e.to_string())
    }
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Evidence {
    pub mechanism: String,
    pub detail: String,
}
impl Evidence {
    pub fn new(mechanism: &str, detail: impl Into<String>) -> Self {
        Self {
            mechanism: mechanism.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}
impl EntryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct Identity {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub modified_ns: i64,
    pub changed_ns: i64,
    pub links: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Entry {
    pub path: String,
    pub parent: String,
    pub name: String,
    pub kind: EntryKind,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub modified_at: Option<i64>,
    pub created_at: Option<i64>,
    pub accessed_at: Option<i64>,
    pub extension: String,
    pub category: String,
    pub confidence: f32,
    pub evidence: Vec<Evidence>,
    pub identity: Identity,
    pub depth: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ScanRequest {
    pub roots: Vec<String>,
    pub exclusions: Vec<String>,
    pub ignore_patterns: Vec<String>,
    pub include_hidden: bool,
    pub cross_filesystems: bool,
}
impl Default for ScanRequest {
    fn default() -> Self {
        Self {
            roots: vec![],
            exclusions: vec![],
            ignore_patterns: vec![],
            include_hidden: true,
            cross_filesystems: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ScanRecord {
    pub id: String,
    pub root: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub status: String,
    pub entries: u64,
    pub warnings: u64,
    pub excluded: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub freshness: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationEvent {
    ScanStarted {
        scan_id: String,
        root: String,
    },
    ScanProgress {
        scan_id: String,
        entries: u64,
        bytes: u64,
    },
    PathIndexed {
        scan_id: String,
        path: String,
    },
    DirectoryCompleted {
        scan_id: String,
        path: String,
        bytes: u64,
    },
    ScanWarning {
        scan_id: String,
        path: String,
        code: String,
        message: String,
    },
    ScanCompleted {
        scan: ScanRecord,
    },
    DuplicateAnalysisStarted {
        operation_id: String,
    },
    DuplicateGroupFound {
        operation_id: String,
        group_id: String,
    },
    DuplicateAnalysisCompleted {
        operation_id: String,
        groups: u64,
    },
    CleanupStarted {
        operation_id: String,
    },
    CleanupProgress {
        operation_id: String,
        path: String,
        status: String,
    },
    CleanupCompleted {
        operation_id: String,
        status: String,
    },
    IndexChanged {
        root: String,
        freshness: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FileQuery {
    pub path: Option<String>,
    pub parent: Option<String>,
    pub name: Option<String>,
    /// Exact names, any of which matches; empty means no name restriction.
    pub names: Vec<String>,
    pub extension: Option<String>,
    pub kind: Option<String>,
    pub category: Option<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub modified_before: Option<i64>,
    pub modified_after: Option<i64>,
    pub created_before: Option<i64>,
    pub created_after: Option<i64>,
    pub sort: String,
    pub limit: u32,
    pub offset: u64,
}
impl Default for FileQuery {
    fn default() -> Self {
        Self {
            path: None,
            parent: None,
            name: None,
            names: vec![],
            extension: None,
            kind: None,
            category: None,
            min_size: None,
            max_size: None,
            modified_before: None,
            modified_after: None,
            created_before: None,
            created_after: None,
            sort: "logical_bytes".into(),
            limit: 100,
            offset: 0,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub limit: u32,
    pub offset: u64,
    pub has_more: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CategoryTotal {
    pub category: String,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub files: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HistoryPoint {
    pub sequence: i64,
    pub scan_id: String,
    pub path: String,
    pub timestamp: i64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub coverage: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DuplicateGroup {
    pub file_count: u64,
    pub id: String,
    pub files: Vec<String>,
    pub file_size: u64,
    pub total_size: u64,
    pub reclaimable_size: u64,
    pub verification: String,
    pub hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DuplicateReport {
    pub analyzed_at: i64,
    pub group_count: u64,
    pub truncated: bool,
    pub operation_id: String,
    pub groups: Vec<DuplicateGroup>,
    pub warnings: Vec<Evidence>,
    pub files_hashed: u64,
    pub cancelled: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Association {
    pub path: String,
    pub kind: String,
    pub bytes: u64,
    pub confidence: String,
    pub evidence: Vec<Evidence>,
    pub selected_by_default: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub bundle_id: Option<String>,
    pub path: String,
    pub footprint_bytes: u64,
    pub associations: Vec<Association>,
    pub coverage: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Insight {
    #[serde(default)]
    pub measurements: InsightMeasurements,
    pub id: String,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub evidence: Vec<Evidence>,
    pub confidence: f32,
    pub severity: String,
    pub estimated_impact: u64,
    pub related_resources: Vec<String>,
    pub possible_actions: Vec<String>,
    pub risk: String,
    pub created_at: i64,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct InsightMeasurements {
    pub logical_bytes: u64,
    pub parent_logical_bytes: Option<u64>,
    pub share_of_parent_percent: Option<f64>,
    pub observation_start: Option<i64>,
    pub observation_end: Option<i64>,
    pub growth_bytes: Option<u64>,
    pub growth_bytes_per_day: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CleanupCandidate {
    pub id: String,
    pub path: String,
    pub size: u64,
    pub category: String,
    pub reason: String,
    pub evidence: Vec<Evidence>,
    pub confidence: f32,
    pub risk: String,
    pub recommended_action: String,
    pub reversible: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlanItem {
    pub path: String,
    pub bytes: u64,
    pub identity: Identity,
    pub content_hash: String,
    pub reason: String,
    pub risk: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CleanupPlan {
    pub id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub items: Vec<PlanItem>,
    pub total_bytes: u64,
    pub action: String,
    pub risk: String,
    pub approval_phrase: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlanRequest {
    pub paths: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExecuteRequest {
    pub approval: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Job {
    pub id: String,
    pub status: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QuarantineItem {
    pub source: String,
    pub destination: String,
    pub identity: Identity,
    pub hash: String,
    pub status: String,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CleanupOperation {
    pub id: String,
    pub plan_id: String,
    pub created_at: i64,
    pub status: String,
    pub items: Vec<QuarantineItem>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AuditRecord {
    pub id: i64,
    pub timestamp: i64,
    pub action: String,
    pub resource_id: String,
    pub detail: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Volume {
    pub name: String,
    pub mount: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub removable: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub disk_read_bytes: u64,
    pub disk_written_bytes: u64,
    pub started_at: u64,
    pub runtime_seconds: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SystemSnapshot {
    pub timestamp: i64,
    pub cpu_percent: f32,
    pub load_average: Vec<f64>,
    pub total_memory: u64,
    pub used_memory: u64,
    pub total_swap: u64,
    pub used_swap: u64,
    pub memory_pressure: Option<String>,
    pub volumes: Vec<Volume>,
    pub processes: Vec<ProcessSnapshot>,
    pub sample_millis: u64,
    pub limitations: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StorageExplanation {
    pub resources: ResourceSummary,
    pub scans: Vec<ScanRecord>,
    pub categories: Vec<CategoryTotal>,
    pub largest_directories: Vec<Entry>,
    pub insights: Vec<Insight>,
    pub history: Vec<HistoryPoint>,
    pub interpretation: String,
    /// Only the currently published generation for each indexed root.
    #[serde(default)]
    pub coverage: Vec<ScanRecord>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DirectoryBreakdown {
    pub directory: Entry,
    pub children: Vec<Entry>,
    pub child_count: u64,
    pub children_logical_bytes: u64,
    pub children_allocated_bytes: u64,
    pub omitted_count: u64,
    pub omitted_logical_bytes: u64,
    pub omitted_allocated_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResourceSummary {
    pub timestamp: i64,
    pub cpu_percent: f32,
    pub used_memory: u64,
    pub total_memory: u64,
    pub volumes: Vec<Volume>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReconcileRequest {
    pub paths: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReconcileReport {
    pub updated_paths: u64,
    pub roots_requiring_scan: Vec<String>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub data_dir: PathBuf,
    pub scan_roots: Vec<String>,
    pub exclusions: Vec<String>,
    pub protected_paths: Vec<PathBuf>,
    pub allowed_cleanup_roots: Vec<PathBuf>,
    pub scan_queue_capacity: usize,
    pub batch_size: usize,
    /// Directory-listing threads for scans; 0 chooses from the machine's core count.
    pub scan_threads: usize,
    pub history_retention_days: u32,
    pub monitoring_interval_seconds: u64,
    pub reconciliation_seconds: u64,
    pub api_bind: String,
}
impl Default for Config {
    fn default() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            data_dir: home.join(".local/share/stratum"),
            scan_roots: vec![],
            exclusions: vec![],
            protected_paths: vec![],
            allowed_cleanup_roots: vec![],
            scan_queue_capacity: 512,
            batch_size: 20000,
            scan_threads: 0,
            history_retention_days: 90,
            monitoring_interval_seconds: 10,
            reconciliation_seconds: 3600,
            api_bind: "127.0.0.1:7391".into(),
        }
    }
}
