//! Application state, background work and key handling for the terminal UI.
//!
//! Every engine call runs on a background thread. Queries carry a revision so a
//! stale result can never overwrite a newer navigation; mutations (scans,
//! duplicate verification, cleanup plans) run one at a time behind `busy`.
use crate::{theme::Theme, widgets, work::LatestRequest};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::{ListState, TableState};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use stratum_engine::{Engine, domain, domain::*};

pub const PAGE_SIZE: u32 = 100;
const MAX_SELECTION: usize = 1000;
const HISTORY_POINTS: usize = 240;
const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Overview,
    Insights,
    Storage,
    Files,
    Apps,
    Duplicates,
    Cleanup,
    System,
    Audit,
}
impl Page {
    pub const ALL: [Self; 9] = [
        Self::Overview,
        Self::Insights,
        Self::Storage,
        Self::Files,
        Self::Apps,
        Self::Duplicates,
        Self::Cleanup,
        Self::System,
        Self::Audit,
    ];
    pub fn number(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0) + 1
    }
    pub fn from_number(number: usize) -> Option<Self> {
        Self::ALL.get(number.wrapping_sub(1)).copied()
    }
    pub fn next(self) -> Self {
        Self::ALL[self.number() % Self::ALL.len()]
    }
    pub fn previous(self) -> Self {
        Self::ALL[(self.number() + Self::ALL.len() - 2) % Self::ALL.len()]
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Insights => "Insights",
            Self::Storage => "Storage",
            Self::Files => "Files",
            Self::Apps => "Applications",
            Self::Duplicates => "Duplicates",
            Self::Cleanup => "Cleanup",
            Self::System => "System",
            Self::Audit => "Audit",
        }
    }
    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Overview => "Your machine, explained. All processing stays on this device.",
            Self::Insights => "Deterministic findings with evidence, confidence, impact and risk.",
            Self::Storage => "Files and folders in proportion. Open a folder to go deeper.",
            Self::Files => "Largest and most recent indexed entries, filtered your way.",
            Self::Apps => "Estimated footprints with evidence for every associated location.",
            Self::Duplicates => {
                "Size → sampled fingerprint → full BLAKE3. You decide which copies matter."
            }
            Self::Cleanup => {
                "Select exact files, review an immutable plan, then explicitly approve."
            }
            Self::System => "A measured resource snapshot, sampled while this page is open.",
            Self::Audit => {
                "A durable local record of indexing, planning, quarantine and restoration."
            }
        }
    }
    pub fn group(self) -> &'static str {
        match self {
            Self::Overview | Self::Insights => "Workspace",
            Self::Storage | Self::Files | Self::Apps | Self::Duplicates => "Storage",
            Self::Cleanup | Self::System | Self::Audit => "Review",
        }
    }
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Overview => "◆",
            Self::Insights => "✦",
            Self::Storage => "▦",
            Self::Files => "≡",
            Self::Apps => "▣",
            Self::Duplicates => "⧉",
            Self::Cleanup => "⊟",
            Self::System => "◔",
            Self::Audit => "≣",
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilesMode {
    Largest,
    Directories,
    Recent,
}
impl FilesMode {
    pub const ALL: [Self; 3] = [Self::Largest, Self::Directories, Self::Recent];
    pub fn label(self) -> &'static str {
        match self {
            Self::Largest => "Largest files",
            Self::Directories => "Largest folders",
            Self::Recent => "Modified this week",
        }
    }
    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcessSort {
    Memory,
    Cpu,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    None,
    Help,
    Scan,
    Progress,
    Roots,
    Operations,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Instant,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CleanupPhase {
    Select,
    Review,
    Outcome,
}
/// One row of the grouped cleanup list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CleanupRow {
    Header { category: String, count: usize },
    Candidate(usize),
}
/// Selection and scroll offset for a list or table, persisted between frames so
/// scrolling stays stable.
#[derive(Default, Clone, Copy, Debug)]
pub struct Nav {
    pub selected: Option<usize>,
    pub offset: usize,
}
impl Nav {
    pub fn index(&self, len: usize) -> Option<usize> {
        if len == 0 {
            None
        } else {
            self.selected.map(|i| i.min(len - 1))
        }
    }
    pub fn ensure(&mut self, len: usize) {
        self.selected = if len == 0 {
            None
        } else {
            Some(self.selected.unwrap_or(0).min(len - 1))
        };
    }
    pub fn step(&mut self, len: usize, delta: i64) {
        if len == 0 {
            self.selected = None;
            return;
        }
        let current = self.index(len).map_or(-1, |i| i as i64);
        let next = if current < 0 && delta < 0 {
            len as i64 - 1
        } else {
            (current + delta).clamp(0, len as i64 - 1)
        };
        self.selected = Some(next as usize);
    }
    pub fn first(&mut self, len: usize) {
        self.selected = (len > 0).then_some(0);
        self.offset = 0;
    }
    pub fn last(&mut self, len: usize) {
        self.selected = len.checked_sub(1);
    }
    pub fn reset(&mut self) {
        self.selected = None;
        self.offset = 0;
    }
    pub fn list_state(&self) -> ListState {
        ListState::default()
            .with_selected(self.selected)
            .with_offset(self.offset)
    }
    pub fn table_state(&self) -> TableState {
        TableState::default()
            .with_selected(self.selected)
            .with_offset(self.offset)
    }
}
#[derive(Default)]
pub struct ScanState {
    pub id: Option<String>,
    pub root: String,
    pub started: Option<Instant>,
    pub entries: u64,
    pub bytes: u64,
    pub warnings: u64,
    pub last_path: String,
    pub paused: bool,
}
impl ScanState {
    pub fn active(&self) -> bool {
        self.started.is_some()
    }
    pub fn elapsed(&self) -> u64 {
        self.started.map_or(0, |t| t.elapsed().as_secs())
    }
}
pub enum Payload {
    Overview(Box<StorageExplanation>),
    Unindexed,
    Breakdown(Box<DirectoryBreakdown>),
    Files(domain::Page<Entry>),
    Insights(Vec<Insight>),
    Apps(Vec<Application>),
    Uninstall {
        id: String,
        name: String,
        reason: String,
    },
    NoDuplicateReport,
    Duplicates(Box<DuplicateReport>),
    DuplicatePage(domain::Page<DuplicateGroup>),
    System(Box<SystemSnapshot>),
    Candidates(domain::Page<CleanupCandidate>, Vec<CleanupOperation>),
    Plan(Box<CleanupPlan>),
    Operation(Box<CleanupOperation>),
    Audit(Vec<AuditRecord>),
    Scanned(Vec<ScanRecord>),
}
pub enum Message {
    Query(u64, Result<Payload>),
    Mutation(Result<Payload>),
}
pub struct App {
    pub engine: Arc<Engine>,
    pub theme: Theme,
    pub page: Page,
    pub overlay: Overlay,
    pub should_quit: bool,
    pub dirty: bool,
    pub tick: u64,
    pub size: (u16, u16),
    tx: mpsc::Sender<Message>,
    rx: mpsc::Receiver<Message>,
    events: tokio::sync::broadcast::Receiver<OperationEvent>,
    queries: LatestRequest,
    pub busy: bool,
    pub status: String,
    pub toast: Option<Toast>,
    pub toast_history: Vec<String>,
    // Overview
    pub overview: Option<StorageExplanation>,
    pub roots: Vec<String>,
    pub findings_nav: Nav,
    // Storage browser
    pub path: String,
    pub breakdown: Option<DirectoryBreakdown>,
    pub storage_nav: Nav,
    pub nav_back: Vec<String>,
    pub roots_nav: Nav,
    // Files
    pub files: Vec<Entry>,
    pub files_mode: FilesMode,
    pub files_offset: u64,
    pub files_has_more: bool,
    pub files_nav: Nav,
    pub filter: String,
    pub filter_draft: String,
    pub filter_editing: bool,
    // Insights
    pub insights: Vec<Insight>,
    pub insights_nav: Nav,
    // Applications
    pub apps: Vec<Application>,
    pub apps_nav: Nav,
    pub uninstall_review: Option<(String, String, String)>,
    // Duplicates
    pub duplicates: Option<DuplicateReport>,
    pub duplicates_offset: u64,
    pub duplicates_has_more: bool,
    pub duplicates_nav: Nav,
    pub duplicate_cancel: Arc<AtomicBool>,
    pub duplicate_running: bool,
    // System
    pub system: Option<SystemSnapshot>,
    pub cpu_history: VecDeque<u64>,
    pub memory_history: VecDeque<u64>,
    pub process_sort: ProcessSort,
    pub process_nav: Nav,
    last_sample: Option<Instant>,
    // Cleanup
    pub candidates: Vec<CleanupCandidate>,
    pub cleanup_rows: Vec<CleanupRow>,
    pub cleanup_nav: Nav,
    pub cleanup_offset: u64,
    pub cleanup_has_more: bool,
    pub cleanup_scope: Option<String>,
    pub selected: BTreeMap<String, u64>,
    pub plan: Option<CleanupPlan>,
    pub operation: Option<CleanupOperation>,
    pub operations: Vec<CleanupOperation>,
    pub operations_nav: Nav,
    pub approval: String,
    // Audit
    pub audit: Vec<AuditRecord>,
    pub audit_offset: u64,
    pub audit_nav: Nav,
    // Scan
    pub scan: ScanState,
    pub scan_input: String,
    pub scan_suggestion: Option<usize>,
}
impl App {
    pub fn new(engine: Arc<Engine>, theme: Theme) -> Self {
        let (tx, rx) = mpsc::channel();
        let events = engine.subscribe();
        let roots = engine.roots().unwrap_or_default();
        let path = roots.first().cloned().unwrap_or_default();
        let mut app = Self {
            engine,
            theme,
            page: Page::Overview,
            overlay: Overlay::None,
            should_quit: false,
            dirty: true,
            tick: 0,
            size: (0, 0),
            tx,
            rx,
            events,
            queries: LatestRequest::default(),
            busy: false,
            status: "Ready · local only".into(),
            toast: None,
            toast_history: Vec::new(),
            overview: None,
            roots,
            findings_nav: Nav::default(),
            path,
            breakdown: None,
            storage_nav: Nav::default(),
            nav_back: Vec::new(),
            roots_nav: Nav::default(),
            files: Vec::new(),
            files_mode: FilesMode::Largest,
            files_offset: 0,
            files_has_more: false,
            files_nav: Nav::default(),
            filter: String::new(),
            filter_draft: String::new(),
            filter_editing: false,
            insights: Vec::new(),
            insights_nav: Nav::default(),
            apps: Vec::new(),
            apps_nav: Nav::default(),
            uninstall_review: None,
            duplicates: None,
            duplicates_offset: 0,
            duplicates_has_more: false,
            duplicates_nav: Nav::default(),
            duplicate_cancel: Arc::new(AtomicBool::new(false)),
            duplicate_running: false,
            system: None,
            cpu_history: VecDeque::with_capacity(HISTORY_POINTS),
            memory_history: VecDeque::with_capacity(HISTORY_POINTS),
            process_sort: ProcessSort::Memory,
            process_nav: Nav::default(),
            last_sample: None,
            candidates: Vec::new(),
            cleanup_rows: Vec::new(),
            cleanup_nav: Nav::default(),
            cleanup_offset: 0,
            cleanup_has_more: false,
            cleanup_scope: None,
            selected: BTreeMap::new(),
            plan: None,
            operation: None,
            operations: Vec::new(),
            operations_nav: Nav::default(),
            approval: String::new(),
            audit: Vec::new(),
            audit_offset: 0,
            audit_nav: Nav::default(),
            scan: ScanState::default(),
            scan_input: String::new(),
            scan_suggestion: None,
        };
        app.refresh();
        app
    }
    pub fn loading(&self) -> bool {
        self.queries.loading()
    }
    /// Whether the interface needs periodic redraws for motion.
    pub fn animating(&self) -> bool {
        self.busy
            || self.loading()
            || self.scan.active()
            || self.duplicate_running
            || self.toast.is_some()
            || self.page == Page::System
    }
    pub fn cleanup_phase(&self) -> CleanupPhase {
        if self.operation.is_some() {
            CleanupPhase::Outcome
        } else if self.plan.is_some() {
            CleanupPhase::Review
        } else {
            CleanupPhase::Select
        }
    }
    pub fn approval_matches(&self) -> bool {
        self.plan
            .as_ref()
            .is_some_and(|p| p.approval_phrase == self.approval && now() < p.expires_at)
    }
    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        let text = text.into();
        self.toast_history.push(text.clone());
        if self.toast_history.len() > 50 {
            self.toast_history.remove(0);
        }
        let seconds = match kind {
            ToastKind::Error => 8,
            ToastKind::Warning => 6,
            _ => 4,
        };
        self.toast = Some(Toast {
            text,
            kind,
            until: Instant::now() + Duration::from_secs(seconds),
        });
        self.dirty = true;
    }
    /// Called roughly every 100 ms while the interface is idle.
    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if self
            .toast
            .as_ref()
            .is_some_and(|t| Instant::now() >= t.until)
        {
            self.toast = None;
            self.dirty = true;
        }
        if self.page == Page::System
            && self.overlay == Overlay::None
            && !self.loading()
            && self
                .last_sample
                .is_none_or(|t| t.elapsed() >= SAMPLE_INTERVAL)
        {
            self.last_sample = Some(Instant::now());
            self.refresh();
        }
    }
    pub fn resize(&mut self, width: u16, height: u16) {
        self.size = (width, height);
        self.dirty = true;
    }
    fn task(&mut self, work: impl FnOnce(Arc<Engine>) -> Result<Payload> + Send + 'static) {
        if self.busy {
            self.toast(
                "Another local operation is still running",
                ToastKind::Warning,
            );
            return;
        }
        self.busy = true;
        self.status = "Working locally…".into();
        let engine = self.engine.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Message::Mutation(work(engine)));
        });
    }
    pub fn refresh(&mut self) {
        self.queries.request();
        self.launch_query();
    }
    fn launch_query(&mut self) {
        let Some(revision) = self.queries.begin() else {
            return;
        };
        let page = self.page;
        let path = self.path.clone();
        let scope = self.cleanup_scope.clone();
        let mode = self.files_mode;
        let filter = self.filter.clone();
        let files_offset = self.files_offset;
        let cleanup_offset = self.cleanup_offset;
        let duplicates_offset = self.duplicates_offset;
        let audit_offset = self.audit_offset;
        let engine = self.engine.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| match page {
                Page::Overview => Ok(Payload::Overview(Box::new(engine.explain_storage()?))),
                Page::Storage if path.is_empty() => Ok(Payload::Unindexed),
                Page::Storage => Ok(Payload::Breakdown(Box::new(
                    engine.directory_breakdown(&path, 60)?,
                ))),
                Page::Files => Ok(Payload::Files(
                    engine.files(&FileQuery {
                        path: (!path.is_empty()).then(|| path.clone()),
                        kind: Some(
                            if mode == FilesMode::Directories {
                                "directory"
                            } else {
                                "file"
                            }
                            .into(),
                        ),
                        modified_after: (mode == FilesMode::Recent).then(|| now() - 7 * 86400),
                        name: (!filter.is_empty()).then(|| filter.clone()),
                        sort: if mode == FilesMode::Recent {
                            "modified_at".into()
                        } else {
                            "logical_bytes".into()
                        },
                        limit: PAGE_SIZE,
                        offset: files_offset,
                        ..Default::default()
                    })?,
                )),
                Page::Insights => Ok(Payload::Insights(engine.insights()?)),
                Page::Apps => Ok(Payload::Apps(engine.applications()?)),
                Page::Duplicates => {
                    if duplicates_offset == 0 {
                        match engine.duplicates() {
                            Ok(report) => Ok(Payload::Duplicates(Box::new(report))),
                            Err(error) if error.code == "not_found" => {
                                Ok(Payload::NoDuplicateReport)
                            }
                            Err(error) => Err(error),
                        }
                    } else {
                        Ok(Payload::DuplicatePage(
                            engine.duplicate_groups(PAGE_SIZE, duplicates_offset)?,
                        ))
                    }
                }
                Page::System => Ok(Payload::System(Box::new(engine.system()))),
                Page::Cleanup => Ok(Payload::Candidates(
                    engine.cleanup_candidates(&FileQuery {
                        path: scope,
                        limit: PAGE_SIZE,
                        offset: cleanup_offset,
                        ..Default::default()
                    })?,
                    engine.cleanup_operations()?,
                )),
                Page::Audit => Ok(Payload::Audit(engine.audit(PAGE_SIZE, audit_offset)?)),
            })();
            let _ = tx.send(Message::Query(revision, result));
        });
    }
    /// Drain engine events and worker results. Returns true when anything changed.
    pub fn receive(&mut self) -> bool {
        let mut changed = false;
        for _ in 0..4096 {
            match self.events.try_recv() {
                Ok(OperationEvent::ScanStarted { scan_id, root }) => {
                    self.scan.id = Some(scan_id);
                    if self.scan.root.is_empty() {
                        self.scan.root = root;
                    }
                    changed = true;
                }
                Ok(OperationEvent::ScanProgress { entries, bytes, .. }) => {
                    self.scan.entries = entries;
                    self.scan.bytes = bytes;
                    changed = true;
                }
                Ok(OperationEvent::PathIndexed { path, .. }) => {
                    self.scan.last_path = path;
                }
                Ok(OperationEvent::ScanWarning { .. }) => {
                    self.scan.warnings += 1;
                    changed = true;
                }
                Ok(OperationEvent::ScanCompleted { scan }) => {
                    self.scan.entries = scan.entries;
                    self.scan.warnings = scan.warnings;
                    changed = true;
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
        while let Ok(message) = self.rx.try_recv() {
            changed = true;
            let result = match message {
                Message::Query(revision, result) => {
                    if !self.queries.finish(revision) {
                        self.launch_query();
                        continue;
                    }
                    result
                }
                Message::Mutation(result) => {
                    self.busy = false;
                    self.duplicate_running = false;
                    let was_scanning = self.scan.active();
                    self.scan = ScanState::default();
                    if was_scanning && self.overlay == Overlay::Progress {
                        self.overlay = Overlay::None;
                    }
                    self.status = "Ready · local only".into();
                    result
                }
            };
            match result {
                Err(error) => {
                    self.status = "Could not complete this request".into();
                    self.toast(error.to_string(), ToastKind::Error);
                }
                Ok(payload) => self.apply(payload),
            }
            self.launch_query();
        }
        if changed {
            self.dirty = true;
        }
        changed
    }
    fn apply(&mut self, payload: Payload) {
        match payload {
            Payload::Unindexed => self.breakdown = None,
            Payload::Overview(value) => {
                self.roots = value.coverage.iter().map(|s| s.root.clone()).collect();
                if self.path.is_empty() {
                    self.path = self.roots.first().cloned().unwrap_or_default();
                }
                self.findings_nav.ensure(value.insights.len().min(6));
                self.overview = Some(*value);
            }
            Payload::Breakdown(value) => {
                let rows = value.children.len() + usize::from(value.omitted_count > 0);
                self.storage_nav.ensure(rows);
                self.breakdown = Some(*value);
            }
            Payload::Files(page) => {
                self.files = page.items;
                self.files_has_more = page.has_more;
                self.files_nav.ensure(self.files.len());
            }
            Payload::Insights(value) => {
                self.insights = value;
                self.insights_nav.ensure(self.insights.len());
            }
            Payload::Apps(value) => {
                self.apps = value;
                self.apps_nav.ensure(self.apps.len());
            }
            Payload::Uninstall { id, name, reason } => {
                self.toast(
                    format!("Uninstall review recorded for {name} · not executable"),
                    ToastKind::Info,
                );
                self.uninstall_review = Some((id, name, reason));
            }
            Payload::NoDuplicateReport => {
                self.duplicates = None;
                self.duplicates_has_more = false;
            }
            Payload::Duplicates(report) => {
                self.duplicates_has_more = report.group_count > report.groups.len() as u64;
                self.duplicates_nav.ensure(report.groups.len());
                self.duplicates = Some(*report);
            }
            Payload::DuplicatePage(page) => {
                self.duplicates_has_more = page.has_more;
                if let Some(report) = self.duplicates.as_mut() {
                    report.groups = page.items;
                    self.duplicates_nav.ensure(report.groups.len());
                }
            }
            Payload::System(snapshot) => {
                let memory = if snapshot.total_memory > 0 {
                    snapshot.used_memory * 100 / snapshot.total_memory
                } else {
                    0
                };
                push_history(&mut self.cpu_history, snapshot.cpu_percent.round() as u64);
                push_history(&mut self.memory_history, memory);
                self.process_nav.ensure(snapshot.processes.len().min(100));
                self.system = Some(*snapshot);
            }
            Payload::Candidates(page, operations) => {
                self.candidates = page.items;
                self.cleanup_has_more = page.has_more;
                self.operations = operations;
                self.operations_nav.ensure(self.operations.len());
                self.rebuild_cleanup_rows();
            }
            Payload::Plan(plan) => {
                self.status = "Plan ready for review · no files moved".into();
                self.plan = Some(*plan);
                self.operation = None;
                self.approval.clear();
                self.toast(
                    "Immutable plan created. Type the approval phrase to authorize.",
                    ToastKind::Info,
                );
            }
            Payload::Operation(operation) => {
                self.status = format!(
                    "Operation {} · inspect the per-file outcome",
                    operation.status
                );
                self.toast(
                    format!(
                        "Operation {} · {} files",
                        widgets::humanize(&operation.status),
                        operation.items.len()
                    ),
                    if operation.status.contains("fail") {
                        ToastKind::Warning
                    } else {
                        ToastKind::Success
                    },
                );
                self.operation = Some(*operation);
                self.plan = None;
                self.approval.clear();
                self.selected.clear();
                self.refresh();
            }
            Payload::Audit(records) => {
                self.audit = records;
                self.audit_nav.ensure(self.audit.len());
            }
            Payload::Scanned(records) => {
                for scan in records {
                    if matches!(scan.status.as_str(), "completed" | "partial")
                        && !self.roots.contains(&scan.root)
                    {
                        self.roots.push(scan.root.clone());
                    }
                    if self.path.is_empty() {
                        self.path = scan.root.clone();
                    }
                    self.toast(
                        format!(
                            "Scan {} · {} entries · {} warnings",
                            scan.status,
                            widgets::count(scan.entries),
                            scan.warnings
                        ),
                        if scan.status == "completed" {
                            ToastKind::Success
                        } else {
                            ToastKind::Warning
                        },
                    );
                }
                self.refresh();
            }
        }
    }
    fn rebuild_cleanup_rows(&mut self) {
        let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (index, candidate) in self.candidates.iter().enumerate() {
            groups
                .entry(candidate.category.as_str())
                .or_default()
                .push(index);
        }
        let mut rows = Vec::new();
        for (category, indexes) in groups {
            rows.push(CleanupRow::Header {
                category: category.to_string(),
                count: indexes.len(),
            });
            rows.extend(indexes.into_iter().map(CleanupRow::Candidate));
        }
        self.cleanup_rows = rows;
        self.cleanup_nav.ensure(self.cleanup_rows.len());
        self.cleanup_skip_header(1);
    }
    fn cleanup_skip_header(&mut self, direction: i64) {
        let len = self.cleanup_rows.len();
        for _ in 0..len {
            match self.cleanup_nav.index(len) {
                Some(i) if matches!(self.cleanup_rows[i], CleanupRow::Header { .. }) => {
                    if (direction < 0 && i == 0) || (direction > 0 && i + 1 >= len) {
                        self.cleanup_nav.step(len, -direction);
                        break;
                    }
                    self.cleanup_nav.step(len, direction);
                }
                _ => break,
            }
        }
    }
    pub fn choose_page(&mut self, page: Page) {
        if self.page == page {
            return;
        }
        self.page = page;
        self.filter_editing = false;
        self.last_sample = (page == Page::System).then(Instant::now);
        self.refresh();
    }
    pub fn browse(&mut self, path: String) {
        if self.path != path && !self.path.is_empty() {
            self.nav_back.push(self.path.clone());
            if self.nav_back.len() > 64 {
                self.nav_back.remove(0);
            }
        }
        self.path = path;
        self.breakdown = None;
        self.storage_nav.reset();
        self.files_offset = 0;
        self.files_nav.reset();
        self.refresh();
    }
    fn parent_within_roots(&self) -> Option<String> {
        if self.roots.contains(&self.path) {
            return None;
        }
        std::path::Path::new(&self.path)
            .parent()
            .map(|p| p.display().to_string())
            .filter(|p| !p.is_empty())
    }
    pub fn open_scan_dialog(&mut self) {
        self.overlay = Overlay::Scan;
        self.scan_suggestion = None;
        if self.scan_input.is_empty() {
            self.scan_input = self.roots.first().cloned().unwrap_or_else(|| {
                std::path::PathBuf::from(home())
                    .join("Downloads")
                    .display()
                    .to_string()
            });
        }
    }
    pub fn scan_suggestions(&self) -> Vec<(String, &'static str)> {
        let home = home();
        let mut out = vec![
            (home.clone(), "Home"),
            (
                std::path::PathBuf::from(&home)
                    .join("Downloads")
                    .display()
                    .to_string(),
                "Downloads",
            ),
            (
                std::path::PathBuf::from(&home)
                    .join("Projects")
                    .display()
                    .to_string(),
                "Projects",
            ),
        ];
        for root in &self.roots {
            if !out.iter().any(|(p, _)| p == root) {
                out.push((root.clone(), "Indexed · rescan"));
            }
        }
        out
    }
    pub fn start_scan(&mut self) {
        let root = self.scan_input.trim().to_string();
        if root.is_empty() {
            self.toast("Choose a folder first", ToastKind::Warning);
            return;
        }
        if self.busy {
            self.toast(
                "Wait for the current operation to finish",
                ToastKind::Warning,
            );
            return;
        }
        self.scan = ScanState {
            root: root.clone(),
            started: Some(Instant::now()),
            ..ScanState::default()
        };
        self.overlay = Overlay::Progress;
        self.task(move |engine| Ok(Payload::Scanned(engine.scan_location(&root)?)));
    }
    fn control_scan(&mut self, action: &str) {
        let Some(id) = self.scan.id.clone() else {
            return;
        };
        match self.engine.control_scan(&id, action) {
            Ok(()) => {
                if action != "cancel" {
                    self.scan.paused = action == "pause";
                }
            }
            Err(error) => self.toast(error.to_string(), ToastKind::Error),
        }
    }
    fn page_len(&self) -> usize {
        self.size.1.saturating_sub(12).max(4) as usize
    }
    /// Keyboard hints for the footer, specific to the current context.
    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        match self.overlay {
            Overlay::Help => return vec![("any key", "close")],
            Overlay::Scan => {
                return vec![
                    ("Enter", "start read-only scan"),
                    ("Tab", "next suggestion"),
                    ("^U", "clear"),
                    ("Esc", "close"),
                ];
            }
            Overlay::Progress => {
                return vec![
                    ("space", if self.scan.paused { "resume" } else { "pause" }),
                    ("c", "cancel scan"),
                    ("Esc", "hide"),
                ];
            }
            Overlay::Roots | Overlay::Operations => {
                return vec![("↑↓", "move"), ("Enter", "open"), ("Esc", "close")];
            }
            Overlay::None => {}
        }
        if self.filter_editing {
            return vec![
                ("type", "filter names"),
                ("Enter", "apply"),
                ("Esc", "cancel"),
            ];
        }
        if self.page == Page::Cleanup && self.cleanup_phase() == CleanupPhase::Review {
            return vec![
                ("type", "approval phrase"),
                ("Enter", "authorize quarantine"),
                ("Esc", "back to selection"),
            ];
        }
        let mut hints = match self.page {
            Page::Overview => vec![("↑↓", "findings"), ("Enter", "investigate")],
            Page::Storage => vec![
                ("↑↓", "move"),
                ("Enter", "open"),
                ("⌫", "up"),
                ("L", "locations"),
                ("c", "candidates here"),
            ],
            Page::Files => vec![
                ("m", "mode"),
                ("/", "filter"),
                ("Enter", "open folder"),
                ("] [", "page"),
                ("L", "locations"),
            ],
            Page::Insights => vec![
                ("↑↓", "move"),
                ("Enter", "investigate"),
                ("c", "candidates"),
            ],
            Page::Apps => vec![("↑↓", "move"), ("u", "uninstall review")],
            Page::Duplicates => {
                if self.duplicate_running {
                    vec![("c", "cancel hashing")]
                } else {
                    vec![("v", "verify content"), ("↑↓", "move"), ("] [", "page")]
                }
            }
            Page::System => vec![("o", "sort"), ("↑↓", "processes")],
            Page::Cleanup => match self.cleanup_phase() {
                CleanupPhase::Select => vec![
                    ("x", "toggle"),
                    ("a", "all on page"),
                    ("p", "plan"),
                    ("o", "operations"),
                    ("] [", "page"),
                ],
                CleanupPhase::Review => vec![],
                CleanupPhase::Outcome => vec![("u", "restore"), ("Esc", "back")],
            },
            Page::Audit => vec![("↑↓", "move"), ("] [", "page")],
        };
        hints.extend([
            (
                "s",
                if self.scan.active() {
                    "scan progress"
                } else {
                    "scan"
                },
            ),
            ("?", "help"),
            ("q", "quit"),
        ]);
        hints
    }
    pub fn handle_key(&mut self, key: KeyEvent) {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }
        self.dirty = true;
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'C'))
        {
            self.should_quit = true;
            return;
        }
        match self.overlay {
            Overlay::Help => {
                self.overlay = Overlay::None;
                return;
            }
            Overlay::Scan => return self.scan_dialog_key(key),
            Overlay::Progress => return self.progress_key(key),
            Overlay::Roots => return self.roots_key(key),
            Overlay::Operations => return self.operations_key(key),
            Overlay::None => {}
        }
        if self.filter_editing {
            return self.filter_key(key);
        }
        if self.page == Page::Cleanup && self.cleanup_phase() == CleanupPhase::Review {
            return self.approval_key(key);
        }
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab => self.choose_page(self.page.next()),
            KeyCode::BackTab => self.choose_page(self.page.previous()),
            KeyCode::Char(c @ '1'..='9') => {
                if let Some(page) = c.to_digit(10).and_then(|d| Page::from_number(d as usize)) {
                    self.choose_page(page);
                }
            }
            KeyCode::Char('s') => {
                if self.scan.active() {
                    self.overlay = Overlay::Progress;
                } else {
                    self.open_scan_dialog();
                }
            }
            KeyCode::Char('r') => {
                self.refresh();
                self.toast("Reading the local index again", ToastKind::Info);
            }
            KeyCode::Char('?') => self.overlay = Overlay::Help,
            _ => self.page_key(key),
        }
    }
    fn page_key(&mut self, key: KeyEvent) {
        let page_len = self.page_len();
        match self.page {
            Page::Overview => {
                let len = self
                    .overview
                    .as_ref()
                    .map_or(0, |o| o.insights.len().min(6));
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => self.findings_nav.step(len, 1),
                    KeyCode::Up | KeyCode::Char('k') => self.findings_nav.step(len, -1),
                    KeyCode::Enter => {
                        if let Some(index) = self.findings_nav.index(len)
                            && let Some(insight) =
                                self.overview.as_ref().and_then(|o| o.insights.get(index))
                        {
                            self.investigate(insight.clone());
                        }
                    }
                    KeyCode::Char('c') => {
                        if let Some(index) = self.findings_nav.index(len)
                            && let Some(insight) =
                                self.overview.as_ref().and_then(|o| o.insights.get(index))
                        {
                            self.review_candidates(insight.related_resources.first().cloned());
                        }
                    }
                    _ => {}
                }
            }
            Page::Storage => self.storage_key(key, page_len),
            Page::Files => self.files_key(key, page_len),
            Page::Insights => {
                let len = self.insights.len();
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => self.insights_nav.step(len, 1),
                    KeyCode::Up | KeyCode::Char('k') => self.insights_nav.step(len, -1),
                    KeyCode::PageDown => self.insights_nav.step(len, page_len as i64),
                    KeyCode::PageUp => self.insights_nav.step(len, -(page_len as i64)),
                    KeyCode::Char('g') | KeyCode::Home => self.insights_nav.first(len),
                    KeyCode::Char('G') | KeyCode::End => self.insights_nav.last(len),
                    KeyCode::Enter => {
                        if let Some(insight) = self
                            .insights_nav
                            .index(len)
                            .and_then(|i| self.insights.get(i))
                        {
                            self.investigate(insight.clone());
                        }
                    }
                    KeyCode::Char('c') => {
                        if let Some(insight) = self
                            .insights_nav
                            .index(len)
                            .and_then(|i| self.insights.get(i))
                        {
                            self.review_candidates(insight.related_resources.first().cloned());
                        }
                    }
                    _ => {}
                }
            }
            Page::Apps => {
                let len = self.apps.len();
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => self.apps_nav.step(len, 1),
                    KeyCode::Up | KeyCode::Char('k') => self.apps_nav.step(len, -1),
                    KeyCode::PageDown => self.apps_nav.step(len, page_len as i64),
                    KeyCode::PageUp => self.apps_nav.step(len, -(page_len as i64)),
                    KeyCode::Char('g') | KeyCode::Home => self.apps_nav.first(len),
                    KeyCode::Char('G') | KeyCode::End => self.apps_nav.last(len),
                    KeyCode::Char('u') => {
                        if let Some(app) = self.apps_nav.index(len).and_then(|i| self.apps.get(i)) {
                            let id = app.id.clone();
                            self.task(move |engine| {
                                let review = engine.uninstall_plan(&id)?;
                                Ok(Payload::Uninstall {
                                    id: review["id"].as_str().unwrap_or_default().to_string(),
                                    name: review["application"]["name"]
                                        .as_str()
                                        .unwrap_or("Application")
                                        .to_string(),
                                    reason: review["reason"]
                                        .as_str()
                                        .unwrap_or("Review the observed associations. No files were changed.")
                                        .to_string(),
                                })
                            });
                        }
                    }
                    _ => {}
                }
            }
            Page::Duplicates => {
                let len = self.duplicates.as_ref().map_or(0, |r| r.groups.len());
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => self.duplicates_nav.step(len, 1),
                    KeyCode::Up | KeyCode::Char('k') => self.duplicates_nav.step(len, -1),
                    KeyCode::PageDown => self.duplicates_nav.step(len, page_len as i64),
                    KeyCode::PageUp => self.duplicates_nav.step(len, -(page_len as i64)),
                    KeyCode::Char('g') | KeyCode::Home => self.duplicates_nav.first(len),
                    KeyCode::Char('G') | KeyCode::End => self.duplicates_nav.last(len),
                    KeyCode::Char('v') => {
                        if !self.busy {
                            self.duplicates_offset = 0;
                            self.duplicate_running = true;
                            self.duplicate_cancel.store(false, Ordering::Relaxed);
                            let cancel = self.duplicate_cancel.clone();
                            self.task(move |engine| {
                                Ok(Payload::Duplicates(Box::new(
                                    engine.discover_duplicates(&cancel)?,
                                )))
                            });
                        }
                    }
                    KeyCode::Char('c') if self.duplicate_running => {
                        self.duplicate_cancel.store(true, Ordering::Relaxed);
                        self.toast("Cancelling content verification", ToastKind::Info);
                    }
                    KeyCode::Char(']') if self.duplicates_has_more => {
                        self.duplicates_offset += u64::from(PAGE_SIZE);
                        self.duplicates_nav.reset();
                        self.refresh();
                    }
                    KeyCode::Char('[') if self.duplicates_offset > 0 => {
                        self.duplicates_offset =
                            self.duplicates_offset.saturating_sub(u64::from(PAGE_SIZE));
                        self.duplicates_nav.reset();
                        self.refresh();
                    }
                    _ => {}
                }
            }
            Page::System => {
                let len = self
                    .system
                    .as_ref()
                    .map_or(0, |s| s.processes.len().min(100));
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => self.process_nav.step(len, 1),
                    KeyCode::Up | KeyCode::Char('k') => self.process_nav.step(len, -1),
                    KeyCode::PageDown => self.process_nav.step(len, page_len as i64),
                    KeyCode::PageUp => self.process_nav.step(len, -(page_len as i64)),
                    KeyCode::Char('g') | KeyCode::Home => self.process_nav.first(len),
                    KeyCode::Char('G') | KeyCode::End => self.process_nav.last(len),
                    KeyCode::Char('o') => {
                        self.process_sort = match self.process_sort {
                            ProcessSort::Memory => ProcessSort::Cpu,
                            ProcessSort::Cpu => ProcessSort::Memory,
                        };
                        self.process_nav.first(len);
                    }
                    _ => {}
                }
            }
            Page::Cleanup => self.cleanup_key(key, page_len),
            Page::Audit => {
                let len = self.audit.len();
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => self.audit_nav.step(len, 1),
                    KeyCode::Up | KeyCode::Char('k') => self.audit_nav.step(len, -1),
                    KeyCode::PageDown => self.audit_nav.step(len, page_len as i64),
                    KeyCode::PageUp => self.audit_nav.step(len, -(page_len as i64)),
                    KeyCode::Char('g') | KeyCode::Home => self.audit_nav.first(len),
                    KeyCode::Char('G') | KeyCode::End => self.audit_nav.last(len),
                    KeyCode::Char(']') if self.audit.len() as u32 >= PAGE_SIZE => {
                        self.audit_offset += u64::from(PAGE_SIZE);
                        self.audit_nav.reset();
                        self.refresh();
                    }
                    KeyCode::Char('[') if self.audit_offset > 0 => {
                        self.audit_offset = self.audit_offset.saturating_sub(u64::from(PAGE_SIZE));
                        self.audit_nav.reset();
                        self.refresh();
                    }
                    _ => {}
                }
            }
        }
    }
    fn storage_key(&mut self, key: KeyEvent, page_len: usize) {
        let (children, omitted) = self
            .breakdown
            .as_ref()
            .map_or((0, 0), |b| (b.children.len(), b.omitted_count));
        let len = children + usize::from(omitted > 0);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.storage_nav.step(len, 1),
            KeyCode::Up | KeyCode::Char('k') => self.storage_nav.step(len, -1),
            KeyCode::PageDown => self.storage_nav.step(len, page_len as i64),
            KeyCode::PageUp => self.storage_nav.step(len, -(page_len as i64)),
            KeyCode::Char('g') | KeyCode::Home => self.storage_nav.first(len),
            KeyCode::Char('G') | KeyCode::End => self.storage_nav.last(len),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                match self.storage_nav.index(len) {
                    Some(index) if index < children => {
                        let entry = &self.breakdown.as_ref().unwrap().children[index];
                        if entry.kind == EntryKind::Directory {
                            let path = entry.path.clone();
                            self.browse(path);
                        } else {
                            self.toast(
                                "Files are inspected in the panel; folders open with Enter",
                                ToastKind::Info,
                            );
                        }
                    }
                    Some(_) => {
                        self.files_mode = FilesMode::Largest;
                        self.files_offset = 0;
                        self.choose_page(Page::Files);
                    }
                    None => {}
                }
            }
            KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
                if let Some(parent) = self.parent_within_roots() {
                    self.browse(parent);
                } else if !self.path.is_empty() {
                    self.toast("Top of this indexed location", ToastKind::Info);
                }
            }
            KeyCode::Char('b') => {
                if let Some(previous) = self.nav_back.pop() {
                    self.path = previous;
                    self.breakdown = None;
                    self.storage_nav.reset();
                    self.refresh();
                }
            }
            KeyCode::Char('L') => {
                self.roots_nav.ensure(self.roots.len());
                self.overlay = Overlay::Roots;
            }
            KeyCode::Char('f') => {
                self.files_offset = 0;
                self.choose_page(Page::Files);
            }
            KeyCode::Char('c') => {
                let scope = match self.storage_nav.index(len) {
                    Some(index) if index < children => {
                        let entry = &self.breakdown.as_ref().unwrap().children[index];
                        if entry.kind == EntryKind::Directory {
                            entry.path.clone()
                        } else {
                            self.path.clone()
                        }
                    }
                    _ => self.path.clone(),
                };
                self.review_candidates(Some(scope));
            }
            _ => {}
        }
    }
    fn files_key(&mut self, key: KeyEvent, page_len: usize) {
        let len = self.files.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.files_nav.step(len, 1),
            KeyCode::Up | KeyCode::Char('k') => self.files_nav.step(len, -1),
            KeyCode::PageDown => self.files_nav.step(len, page_len as i64),
            KeyCode::PageUp => self.files_nav.step(len, -(page_len as i64)),
            KeyCode::Char('g') | KeyCode::Home => self.files_nav.first(len),
            KeyCode::Char('G') | KeyCode::End => self.files_nav.last(len),
            KeyCode::Char('m') => {
                self.files_mode = self.files_mode.next();
                self.files_offset = 0;
                self.files_nav.reset();
                self.refresh();
            }
            KeyCode::Char('/') => {
                self.filter_draft = self.filter.clone();
                self.filter_editing = true;
            }
            KeyCode::Enter => {
                if let Some(entry) = self.files_nav.index(len).and_then(|i| self.files.get(i)) {
                    let target = if entry.kind == EntryKind::Directory {
                        entry.path.clone()
                    } else {
                        entry.parent.clone()
                    };
                    self.page = Page::Storage;
                    self.browse(target);
                }
            }
            KeyCode::Char(']') if self.files_has_more => {
                self.files_offset += u64::from(PAGE_SIZE);
                self.files_nav.reset();
                self.refresh();
            }
            KeyCode::Char('[') if self.files_offset > 0 => {
                self.files_offset = self.files_offset.saturating_sub(u64::from(PAGE_SIZE));
                self.files_nav.reset();
                self.refresh();
            }
            KeyCode::Char('L') => {
                self.roots_nav.ensure(self.roots.len());
                self.overlay = Overlay::Roots;
            }
            _ => {}
        }
    }
    fn cleanup_key(&mut self, key: KeyEvent, page_len: usize) {
        if self.cleanup_phase() == CleanupPhase::Outcome {
            match key.code {
                KeyCode::Char('u') => {
                    if let Some(operation) = self.operation.clone()
                        && operation.items.iter().any(|i| i.status != "restored")
                    {
                        self.task(move |engine| {
                            Ok(Payload::Operation(Box::new(
                                engine.undo_cleanup(&operation.id)?,
                            )))
                        });
                    } else {
                        self.toast("Nothing left to restore in this operation", ToastKind::Info);
                    }
                }
                KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('b') => {
                    self.operation = None;
                    self.refresh();
                }
                KeyCode::Char('o') => {
                    self.operations_nav.ensure(self.operations.len());
                    self.overlay = Overlay::Operations;
                }
                _ => {}
            }
            return;
        }
        let len = self.cleanup_rows.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.cleanup_nav.step(len, 1);
                self.cleanup_skip_header(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.cleanup_nav.step(len, -1);
                self.cleanup_skip_header(-1);
            }
            KeyCode::PageDown => {
                self.cleanup_nav.step(len, page_len as i64);
                self.cleanup_skip_header(1);
            }
            KeyCode::PageUp => {
                self.cleanup_nav.step(len, -(page_len as i64));
                self.cleanup_skip_header(-1);
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.cleanup_nav.first(len);
                self.cleanup_skip_header(1);
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.cleanup_nav.last(len);
                self.cleanup_skip_header(-1);
            }
            KeyCode::Char('x' | ' ') | KeyCode::Enter => {
                if let Some(CleanupRow::Candidate(index)) = self
                    .cleanup_nav
                    .index(len)
                    .map(|i| self.cleanup_rows[i].clone())
                    && let Some(candidate) = self.candidates.get(index)
                {
                    if self.selected.remove(&candidate.path).is_none() {
                        if self.selected.len() >= MAX_SELECTION {
                            self.toast(
                                "A plan reviews at most 1,000 files at once",
                                ToastKind::Warning,
                            );
                        } else {
                            self.selected.insert(candidate.path.clone(), candidate.size);
                        }
                    }
                    self.cleanup_nav.step(len, 1);
                    self.cleanup_skip_header(1);
                }
            }
            KeyCode::Char('a') => {
                let all_selected = !self.candidates.is_empty()
                    && self
                        .candidates
                        .iter()
                        .all(|c| self.selected.contains_key(&c.path));
                if all_selected {
                    for candidate in &self.candidates {
                        self.selected.remove(&candidate.path);
                    }
                } else {
                    for candidate in &self.candidates {
                        if self.selected.len() < MAX_SELECTION {
                            self.selected.insert(candidate.path.clone(), candidate.size);
                        }
                    }
                }
            }
            KeyCode::Char('n') => self.selected.clear(),
            KeyCode::Char('p') => {
                if self.selected.is_empty() {
                    self.toast("Select at least one exact file first", ToastKind::Warning);
                } else {
                    let paths = self.selected.keys().cloned().collect();
                    self.task(move |engine| {
                        Ok(Payload::Plan(Box::new(
                            engine.create_cleanup_plan(PlanRequest { paths })?,
                        )))
                    });
                }
            }
            KeyCode::Char('o') => {
                self.operations_nav.ensure(self.operations.len());
                self.overlay = Overlay::Operations;
            }
            KeyCode::Char('0') if self.cleanup_scope.is_some() => {
                self.cleanup_scope = None;
                self.cleanup_offset = 0;
                self.refresh();
            }
            KeyCode::Char(']') if self.cleanup_has_more => {
                self.cleanup_offset += u64::from(PAGE_SIZE);
                self.cleanup_nav.reset();
                self.refresh();
            }
            KeyCode::Char('[') if self.cleanup_offset > 0 => {
                self.cleanup_offset = self.cleanup_offset.saturating_sub(u64::from(PAGE_SIZE));
                self.cleanup_nav.reset();
                self.refresh();
            }
            _ => {}
        }
    }
    fn approval_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.plan = None;
                self.approval.clear();
                self.toast("Plan discarded · nothing was moved", ToastKind::Info);
            }
            KeyCode::Enter => {
                if let Some(plan) = self.plan.clone() {
                    if now() >= plan.expires_at {
                        self.toast(
                            "This plan has expired. Create a new one.",
                            ToastKind::Warning,
                        );
                    } else if self.approval == plan.approval_phrase {
                        let approval = self.approval.clone();
                        self.task(move |engine| {
                            Ok(Payload::Operation(Box::new(
                                engine.execute_cleanup_plan(&plan.id, &approval)?,
                            )))
                        });
                    } else {
                        self.toast("The approval phrase must match exactly", ToastKind::Warning);
                    }
                }
            }
            KeyCode::Backspace => {
                self.approval.pop();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.approval.clear();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.approval.len() < 200 {
                    self.approval.push(c);
                }
            }
            _ => {}
        }
    }
    fn filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.filter_editing = false,
            KeyCode::Enter => {
                self.filter_editing = false;
                self.filter = self.filter_draft.trim().to_string();
                self.files_offset = 0;
                self.files_nav.reset();
                self.refresh();
            }
            KeyCode::Backspace => {
                self.filter_draft.pop();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter_draft.clear();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.filter_draft.len() < 120 {
                    self.filter_draft.push(c);
                }
            }
            _ => {}
        }
    }
    fn scan_dialog_key(&mut self, key: KeyEvent) {
        let suggestions = self.scan_suggestions();
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Enter => self.start_scan(),
            KeyCode::Tab | KeyCode::Down => {
                let next = self
                    .scan_suggestion
                    .map_or(0, |i| (i + 1) % suggestions.len().max(1));
                self.scan_suggestion = Some(next);
                if let Some((path, _)) = suggestions.get(next) {
                    self.scan_input = path.clone();
                }
            }
            KeyCode::BackTab | KeyCode::Up => {
                let len = suggestions.len().max(1);
                let next = self
                    .scan_suggestion
                    .map_or(len - 1, |i| (i + len - 1) % len);
                self.scan_suggestion = Some(next);
                if let Some((path, _)) = suggestions.get(next) {
                    self.scan_input = path.clone();
                }
            }
            KeyCode::Backspace => {
                self.scan_input.pop();
                self.scan_suggestion = None;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scan_input.clear();
                self.scan_suggestion = None;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.scan_input.len() < 1024 {
                    self.scan_input.push(c);
                    self.scan_suggestion = None;
                }
            }
            _ => {}
        }
    }
    fn progress_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Char(' ') => {
                let action = if self.scan.paused { "resume" } else { "pause" };
                self.control_scan(action);
            }
            KeyCode::Char('c') => {
                self.engine.cancel_all();
                self.toast(
                    "Cancelling the scan · indexed data stays unpublished",
                    ToastKind::Info,
                );
            }
            _ => {}
        }
    }
    fn roots_key(&mut self, key: KeyEvent) {
        let len = self.roots.len();
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Down | KeyCode::Char('j') => self.roots_nav.step(len, 1),
            KeyCode::Up | KeyCode::Char('k') => self.roots_nav.step(len, -1),
            KeyCode::Enter => {
                if let Some(root) = self.roots_nav.index(len).and_then(|i| self.roots.get(i)) {
                    let root = root.clone();
                    self.overlay = Overlay::None;
                    self.browse(root);
                }
            }
            _ => {}
        }
    }
    fn operations_key(&mut self, key: KeyEvent) {
        let len = self.operations.len();
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Down | KeyCode::Char('j') => self.operations_nav.step(len, 1),
            KeyCode::Up | KeyCode::Char('k') => self.operations_nav.step(len, -1),
            KeyCode::Enter => {
                if let Some(operation) = self
                    .operations_nav
                    .index(len)
                    .and_then(|i| self.operations.get(i))
                {
                    let id = operation.id.clone();
                    self.overlay = Overlay::None;
                    self.task(move |engine| {
                        Ok(Payload::Operation(Box::new(engine.cleanup_operation(&id)?)))
                    });
                }
            }
            _ => {}
        }
    }
    fn investigate(&mut self, insight: Insight) {
        let Some(path) = insight.related_resources.first() else {
            return;
        };
        let target = if insight.possible_actions.iter().any(|a| a == "inspect_file") {
            std::path::Path::new(path)
                .parent()
                .map_or_else(|| path.clone(), |p| p.display().to_string())
        } else {
            path.clone()
        };
        self.page = Page::Storage;
        self.browse(target);
    }
    fn review_candidates(&mut self, scope: Option<String>) {
        self.cleanup_scope = scope;
        self.cleanup_offset = 0;
        self.selected.clear();
        self.plan = None;
        self.operation = None;
        self.cleanup_nav.reset();
        self.page = Page::Cleanup;
        self.refresh();
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.engine.cancel_all();
        self.duplicate_cancel.store(true, Ordering::Relaxed);
    }
}
fn push_history(history: &mut VecDeque<u64>, value: u64) {
    if history.len() >= HISTORY_POINTS {
        history.pop_front();
    }
    history.push_back(value);
}
pub fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pages_cycle_in_sidebar_order() {
        assert_eq!(Page::Overview.number(), 1);
        assert_eq!(Page::Audit.number(), 9);
        assert_eq!(Page::from_number(3), Some(Page::Storage));
        assert_eq!(Page::from_number(0), None);
        assert_eq!(Page::from_number(10), None);
        assert_eq!(Page::Audit.next(), Page::Overview);
        assert_eq!(Page::Overview.previous(), Page::Audit);
        let mut page = Page::Overview;
        for _ in 0..Page::ALL.len() {
            page = page.next();
        }
        assert_eq!(page, Page::Overview);
    }
    #[test]
    fn navigation_clamps_and_wraps_sensibly() {
        let mut nav = Nav::default();
        assert_eq!(nav.index(0), None);
        nav.step(5, 1);
        assert_eq!(nav.selected, Some(0));
        nav.step(5, 10);
        assert_eq!(nav.selected, Some(4));
        nav.step(5, -1);
        assert_eq!(nav.selected, Some(3));
        nav.step(2, 1);
        assert_eq!(nav.index(2), Some(1));
        nav.step(0, 1);
        assert_eq!(nav.selected, None);
        nav.step(3, -1);
        assert_eq!(nav.selected, Some(2));
        nav.ensure(1);
        assert_eq!(nav.selected, Some(0));
    }
    #[test]
    fn files_modes_cycle() {
        assert_eq!(FilesMode::Largest.next(), FilesMode::Directories);
        assert_eq!(FilesMode::Recent.next(), FilesMode::Largest);
    }
}
