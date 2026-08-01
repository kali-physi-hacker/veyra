mod cleanup;
mod design;
mod overview;
mod storage;
#[cfg(test)]
mod ui_tests;
mod work;
use clap::Parser;
use design::*;
use eframe::egui::{self, Color32, RichText, Vec2};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::AtomicBool, mpsc},
};
use stratum_domain::*;
use stratum_engine::Engine;

const TEAL: Color32 = Color32::from_rgb(72, 214, 184);
const MUTED: Color32 = Color32::from_rgb(139, 154, 177);
const PANEL: Color32 = Color32::from_rgb(26, 30, 43);
const COLORS: [Color32; 6] = [
    TEAL,
    Color32::from_rgb(104, 151, 232),
    Color32::from_rgb(171, 142, 235),
    Color32::from_rgb(231, 180, 99),
    Color32::from_rgb(223, 128, 144),
    Color32::from_rgb(92, 176, 199),
];
#[derive(Parser)]
struct Options {
    #[arg(long, env = "STRATUM_DATA_DIR")]
    data_dir: Option<std::path::PathBuf>,
    #[arg(long)]
    config: Option<std::path::PathBuf>,
    #[arg(long, value_enum, default_value = "overview")]
    page: Page,
}
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Page {
    Overview,
    Explorer,
    Map,
    Cleanup,
    Apps,
    Duplicates,
    System,
    Insights,
    History,
    Audit,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Explorer => "Storage explorer",
            Self::Map => "Storage map",
            Self::Cleanup => "Cleanup planner",
            Self::Apps => "Applications",
            Self::Duplicates => "Duplicates",
            Self::System => "System monitor",
            Self::Insights => "Insights",
            Self::History => "Storage history",
            Self::Audit => "Activity & audit",
        }
    }
    fn subtitle(self) -> &'static str {
        match self {
            Self::Overview => "Your machine, explained. All processing stays on this device.",
            Self::Explorer => {
                "Explore indexed paths. Open a directory to see its immediate children."
            }
            Self::Map => {
                "Files and folders, in proportion. Inspect an item or open a folder to go deeper."
            }
            Self::Cleanup => {
                "Select specific files, review an immutable plan, then explicitly approve."
            }
            Self::Apps => "Estimated footprints with evidence for every associated location.",
            Self::Duplicates => {
                "Size → sampled fingerprint → full BLAKE3. You decide which copies matter."
            }
            Self::System => "A measured resource snapshot. Refresh to sample again.",
            Self::Insights => "Deterministic findings with evidence, confidence, impact and risk.",
            Self::History => {
                "Observed directory totals over time. Missing periods are not inferred."
            }
            Self::Audit => {
                "A durable local record of indexing, planning, quarantine and restoration."
            }
        }
    }
}
enum Payload {
    Unindexed,
    NoDuplicateReport,
    Overview(StorageExplanation),
    Files(domain::Page<Entry>),
    Candidates(domain::Page<CleanupCandidate>, Vec<CleanupOperation>),
    Breakdown(DirectoryBreakdown),
    Apps(Vec<Application>),
    Duplicates(DuplicateReport),
    DuplicatePage(domain::Page<DuplicateGroup>),
    Uninstall(serde_json::Value),
    System(SystemSnapshot),
    Insights(Vec<Insight>),
    History(Vec<HistoryPoint>),
    Audit(Vec<AuditRecord>),
    Plan(CleanupPlan),
    Operation(CleanupOperation),
    Scanned(Vec<ScanRecord>),
}
enum Message {
    Query(u64, Result<Payload>),
    Mutation(Result<Payload>),
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum FileMode {
    Children,
    Largest,
    Recent,
}
use stratum_engine::domain;
struct App {
    engine: Arc<Engine>,
    page: Page,
    tx: mpsc::Sender<Message>,
    rx: mpsc::Receiver<Message>,
    ctx: egui::Context,
    queries: work::LatestRequest,
    events: tokio::sync::broadcast::Receiver<OperationEvent>,
    busy: bool,
    status: String,
    error: Option<String>,
    root: String,
    path: String,
    offset: u64,
    has_more: bool,
    sort: String,
    search: String,
    overview: Option<StorageExplanation>,
    files: Vec<Entry>,
    candidates: Vec<CleanupCandidate>,
    apps: Vec<Application>,
    duplicates: Option<DuplicateReport>,
    system: Option<SystemSnapshot>,
    insights: Vec<Insight>,
    history: Vec<HistoryPoint>,
    audit: Vec<AuditRecord>,
    selected: BTreeMap<String, u64>,
    plan: Option<CleanupPlan>,
    operation: Option<CleanupOperation>,
    approval: String,
    operation_lookup: String,
    uninstall: Option<serde_json::Value>,
    breakdown: Option<DirectoryBreakdown>,
    selected_entry: Option<Entry>,
    hovered_path: Option<String>,
    file_mode: FileMode,
    nav_back: Vec<String>,
    nav_forward: Vec<String>,
    show_scan_dialog: bool,
    indexed_roots: Vec<String>,
    cleanup_scope: Option<String>,
    operations: Vec<CleanupOperation>,
    active_scan: Option<String>,
    scan_started: Option<std::time::Instant>,
    paused: bool,
    duplicate_cancel: Arc<AtomicBool>,
    duplicate_running: bool,
}
impl App {
    fn new(ctx: &egui::Context, engine: Arc<Engine>, page: Page) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(14, 20, 30);
        visuals.window_fill = PANEL;
        visuals.override_text_color = Some(Color32::from_rgb(226, 233, 242));
        visuals.selection.bg_fill = Color32::from_rgb(32, 80, 75);
        visuals.selection.stroke.color = TEAL;
        visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1.0, Color32::from_rgb(43, 55, 72));
        ctx.set_visuals(visuals);
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(12.0, 12.0);
        style.spacing.button_padding = Vec2::new(12.0, 8.0);
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(28.0));
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        ctx.set_style(style);
        let (tx, rx) = mpsc::channel();
        let events = engine.subscribe();
        let indexed_roots = engine.roots().unwrap_or_default();
        let indexed_root = indexed_roots.first().cloned();
        let root = indexed_root
            .clone()
            .unwrap_or_else(|| std::env::var("HOME").unwrap_or_default());
        let mut app = Self {
            engine,
            page,
            tx,
            rx,
            ctx: ctx.clone(),
            queries: work::LatestRequest::default(),
            events,
            busy: false,
            status: "Ready · local only".into(),
            error: None,
            root,
            path: indexed_root.unwrap_or_default(),
            offset: 0,
            has_more: false,
            sort: "logical_bytes".into(),
            search: String::new(),
            overview: None,
            files: vec![],
            candidates: vec![],
            apps: vec![],
            duplicates: None,
            system: None,
            insights: vec![],
            history: vec![],
            audit: vec![],
            selected: BTreeMap::new(),
            plan: None,
            operation: None,
            approval: String::new(),
            operation_lookup: String::new(),
            uninstall: None,
            breakdown: None,
            selected_entry: None,
            hovered_path: None,
            file_mode: FileMode::Children,
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            show_scan_dialog: false,
            indexed_roots,
            cleanup_scope: None,
            operations: Vec::new(),
            active_scan: None,
            scan_started: None,
            paused: false,
            duplicate_cancel: Arc::new(AtomicBool::new(false)),
            duplicate_running: false,
        };
        app.refresh();
        app
    }
    fn task(&mut self, work: impl FnOnce(Arc<Engine>) -> Result<Payload> + Send + 'static) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        self.status = "Working locally…".into();
        let engine = self.engine.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Message::Mutation(work(engine)));
            ctx.request_repaint();
        });
    }
    fn refresh(&mut self) {
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
        let offset = self.offset;
        let sort = self.sort.clone();
        let search = self.search.clone();
        let mode = self.file_mode;
        let engine = self.engine.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let e = engine;
                match page {
                    Page::Overview => Ok(Payload::Overview(e.explain_storage()?)),
                    Page::Explorer => Ok(Payload::Files(e.files(&FileQuery {
                        parent: if mode == FileMode::Children && !path.is_empty() {
                            Some(path.clone())
                        } else {
                            None
                        },
                        path: if mode != FileMode::Children && !path.is_empty() {
                            Some(path)
                        } else {
                            None
                        },
                        kind: if mode == FileMode::Children {
                            None
                        } else {
                            Some("file".into())
                        },
                        modified_after: if mode == FileMode::Recent {
                            Some(now() - 7 * 86400)
                        } else {
                            None
                        },
                        name: if search.is_empty() {
                            None
                        } else {
                            Some(search)
                        },
                        sort: if mode == FileMode::Recent {
                            "modified_at".into()
                        } else {
                            sort
                        },
                        limit: 100,
                        offset,
                        ..Default::default()
                    })?)),
                    Page::Map if path.is_empty() => Ok(Payload::Unindexed),
                    Page::Map => Ok(Payload::Breakdown(e.directory_breakdown(&path, 60)?)),
                    Page::Cleanup => Ok(Payload::Candidates(
                        e.cleanup_candidates(&FileQuery {
                            path: scope,
                            limit: 100,
                            offset,
                            ..Default::default()
                        })?,
                        e.cleanup_operations()?,
                    )),
                    Page::Apps => Ok(Payload::Apps(e.applications()?)),
                    Page::Duplicates => {
                        if offset == 0 {
                            match e.duplicates() {
                                Ok(report) => Ok(Payload::Duplicates(report)),
                                Err(error) if error.code == "not_found" => {
                                    Ok(Payload::NoDuplicateReport)
                                }
                                Err(error) => Err(error),
                            }
                        } else {
                            Ok(Payload::DuplicatePage(e.duplicate_groups(100, offset)?))
                        }
                    }
                    Page::System => Ok(Payload::System(e.system())),
                    Page::Insights => Ok(Payload::Insights(e.insights()?)),
                    Page::History => Ok(Payload::History(
                        e.history(if path.is_empty() { None } else { Some(&path) }, 0)?,
                    )),
                    Page::Audit => Ok(Payload::Audit(e.audit(100, offset)?)),
                }
            })();
            let _ = tx.send(Message::Query(revision, result));
            ctx.request_repaint();
        });
    }
    fn receive(&mut self) {
        for _ in 0..1024 {
            match self.events.try_recv() {
                Ok(OperationEvent::ScanStarted { scan_id, .. }) => {
                    self.active_scan = Some(scan_id);
                }
                Ok(OperationEvent::ScanProgress {
                    scan_id,
                    entries,
                    bytes: count,
                    ..
                }) => {
                    self.active_scan = Some(scan_id);
                    self.status = format!(
                        "Indexed {entries} entries · {} discovered · {}s elapsed",
                        bytes(count),
                        self.scan_started.map_or(0, |t| t.elapsed().as_secs())
                    );
                }
                Ok(OperationEvent::ScanCompleted { scan }) => {
                    self.active_scan = None;
                    self.paused = false;
                    self.status = format!(
                        "{} · {} entries · {} warnings",
                        scan.status, scan.entries, scan.warnings
                    );
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
        while let Ok(message) = self.rx.try_recv() {
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
                    self.active_scan = None;
                    self.scan_started = None;
                    result
                }
            };
            match result {
                Err(e) => {
                    self.error = Some(e.to_string());
                    self.status = "Could not complete this request".into();
                }
                Ok(payload) => match payload {
                    Payload::Unindexed => self.breakdown = None,
                    Payload::NoDuplicateReport => {
                        self.duplicates = None;
                        self.has_more = false;
                    }
                    Payload::Overview(v) => {
                        self.indexed_roots = v.coverage.iter().map(|s| s.root.clone()).collect();
                        self.overview = Some(v);
                    }
                    Payload::Files(v) => {
                        self.files = v.items;
                        self.has_more = v.has_more;
                    }
                    Payload::Breakdown(v) => {
                        self.breakdown = Some(v);
                    }
                    Payload::Candidates(v, operations) => {
                        self.candidates = v.items;
                        self.has_more = v.has_more;
                        self.operations = operations;
                    }
                    Payload::Apps(v) => self.apps = v,
                    Payload::Duplicates(v) => {
                        self.has_more = v.group_count > v.groups.len() as u64;
                        self.duplicates = Some(v);
                    }
                    Payload::DuplicatePage(v) => {
                        self.has_more = v.has_more;
                        if let Some(report) = self.duplicates.as_mut() {
                            report.groups = v.items;
                        }
                    }
                    Payload::Uninstall(v) => self.uninstall = Some(v),
                    Payload::System(v) => self.system = Some(v),
                    Payload::Insights(v) => self.insights = v,
                    Payload::History(v) => self.history = v,
                    Payload::Audit(v) => self.audit = v,
                    Payload::Plan(v) => {
                        self.status = "Plan ready for review · no files moved".into();
                        self.plan = Some(v);
                        self.operation = None;
                        self.approval.clear();
                    }
                    Payload::Operation(v) => {
                        self.status =
                            format!("Operation {} · inspect the per-file outcome", v.status);
                        self.operation_lookup = v.id.clone();
                        self.operation = Some(v);
                        self.plan = None;
                        self.approval.clear();
                        self.selected.clear();
                        self.refresh();
                    }
                    Payload::Scanned(records) => {
                        for scan in records {
                            if matches!(scan.status.as_str(), "completed" | "partial")
                                && !self.indexed_roots.contains(&scan.root)
                            {
                                self.indexed_roots.push(scan.root.clone());
                            }
                            if self.path.is_empty() {
                                self.path = scan.root;
                            }
                        }
                        self.refresh();
                    }
                },
            }
            self.launch_query();
        }
    }
    fn choose_page(&mut self, page: Page) {
        if self.page == page {
            return;
        }
        self.page = page;
        self.offset = 0;
        self.has_more = false;
        self.error = None;
        self.refresh();
    }
    fn start_scan(&mut self) {
        if self.busy || self.root.is_empty() {
            return;
        }
        self.show_scan_dialog = false;
        self.scan_started = Some(std::time::Instant::now());
        self.paused = false;
        let root = self.root.clone();
        self.task(move |e| Ok(Payload::Scanned(e.scan_location(&root)?)));
    }
    fn reveal(&mut self, path: &str) {
        #[cfg(target_os = "macos")]
        if let Err(error) = std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(path)
            .spawn()
        {
            self.error = Some(error.to_string());
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = path;
            self.error =
                Some("Reveal in file manager is currently supported on macOS only.".into());
        }
    }
    fn metric(ui: &mut egui::Ui, label: &str, value: String, detail: &str) {
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(12)
            .inner_margin(18)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(175.0);
                    ui.label(RichText::new(label.to_uppercase()).small().color(MUTED));
                    ui.label(RichText::new(value).size(29.0).strong());
                    ui.label(RichText::new(detail).small().color(MUTED))
                        .on_hover_text(detail);
                });
            });
    }
    fn apps(&mut self, ui: &mut egui::Ui) {
        if self.apps.is_empty() && !self.queries.loading() {
            empty(
                ui,
                "No indexed application bundles",
                "Scan /Applications and relevant Library locations. Footprints only include paths actually observed.",
            );
        }
        let mut requested = None;
        let mut reveal = None;
        for app in self.apps.iter().take(100) {
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&app.name).size(23.0).strong());
                    pill(ui, &bytes(app.footprint_bytes), ACCENT);
                    pill(ui, "Estimated footprint", MUTED);
                });
                if let Some(id) = &app.bundle_id {
                    ui.label(RichText::new(id).monospace().small().color(MUTED));
                }
                ui.label(RichText::new(&app.coverage).small().color(MUTED));
                egui::CollapsingHeader::new(format!(
                    "{} observed storage locations",
                    app.associations.len()
                ))
                .id_salt(&app.id)
                .show(ui, |ui| {
                    for association in &app.associations {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(&association.kind);
                            ui.label(bytes(association.bytes));
                            pill(ui, &association.confidence, TEAL);
                        });
                        ui.label(short_path(&association.path))
                            .on_hover_text(&association.path);
                        for evidence in &association.evidence {
                            ui.label(RichText::new(&evidence.detail).small().color(MUTED));
                        }
                        ui.separator();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Reveal application").clicked() {
                        reveal = Some(app.path.clone());
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Create uninstall review"))
                        .clicked()
                    {
                        requested = Some(app.id.clone());
                    }
                });
            });
        }
        if self.apps.len() > 100 {
            ui.label("Showing the largest 100 indexed applications; the API exposes the full bounded inventory.");
        }
        if let Some(path) = reveal {
            self.reveal(&path);
        }
        if let Some(id) = requested {
            self.task(move |e| Ok(Payload::Uninstall(e.uninstall_plan(&id)?)));
        }
        if let Some(review) = &self.uninstall {
            card().show(ui, |ui| {
                eyebrow(ui, "Uninstall review · not executable");
                ui.strong(
                    review["application"]["name"]
                        .as_str()
                        .unwrap_or("Application"),
                );
                ui.label(
                    review["reason"]
                        .as_str()
                        .unwrap_or("Review the observed associations. No files were changed."),
                );
                if let Some(id) = review["id"].as_str() {
                    ui.horizontal(|ui| {
                        ui.monospace(id);
                        if ui.small_button("Copy report ID").clicked() {
                            ui.ctx().copy_text(id.into());
                        }
                    });
                }
            });
        }
        ui.add_space(12.0);
        ui.label(RichText::new("Bundle removal is not supported. Association evidence is not proof that shared data can be discarded.").small().color(AMBER));
    }
    fn duplicates(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Verify duplicate content").fill(ACCENT.gamma_multiply(0.3)),
                )
                .clicked()
            {
                self.offset = 0;
                self.duplicate_running = true;
                self.duplicate_cancel
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                let cancel = self.duplicate_cancel.clone();
                self.task(move |e| Ok(Payload::Duplicates(e.discover_duplicates(&cancel)?)));
            }
            ui.label(
                RichText::new("Size → sample → full BLAKE3 verification")
                    .small()
                    .color(MUTED),
            );
        });
        let mut reveal = None;
        if let Some(report) = &self.duplicates {
            card().show(ui,|ui|{
                ui.set_width(ui.available_width());
                ui.label(RichText::new(format!("{} duplicate groups",report.group_count)).size(26.0).strong());
                ui.label(format!("{} files fully hashed · {} warnings · observed {}",report.files_hashed,report.warnings.len(),age(report.analyzed_at)));
                ui.label(RichText::new("These are observations from the last analysis, not a live guarantee. Sparse files and shared blocks affect potential physical savings. No copy is selected for deletion.").small().color(MUTED));
            });
            if report.groups.is_empty() {
                empty(
                    ui,
                    "No groups in this report",
                    "Run verification after indexing files. Cancellation and permission warnings may limit the report.",
                );
            }
            for group in &report.groups {
                card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let name = group
                        .files
                        .first()
                        .and_then(|p| std::path::Path::new(p).file_name())
                        .map_or_else(
                            || "Duplicate group".into(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                    ui.strong(name);
                    ui.horizontal_wrapped(|ui| {
                        pill(ui, &format!("{} copies", group.file_count), ACCENT);
                        ui.label(format!(
                            "{} each · {} logically redundant",
                            bytes(group.file_size),
                            bytes(group.reclaimable_size)
                        ));
                    });
                    egui::CollapsingHeader::new("Compare locations")
                        .id_salt(&group.id)
                        .show(ui, |ui| {
                            for path in &group.files {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(short_path(path)).on_hover_text(path);
                                    if ui.small_button("Reveal").clicked() {
                                        reveal = Some(path.clone());
                                    }
                                });
                            }
                            if group.file_count > group.files.len() as u64 {
                                ui.label(format!(
                                    "{} paths shown of {}",
                                    group.files.len(),
                                    group.file_count
                                ));
                            }
                            ui.label(RichText::new(&group.verification).small().color(MUTED));
                        });
                });
            }
            for warning in &report.warnings {
                ui.label(
                    RichText::new(format!("{}: {}", warning.mechanism, warning.detail))
                        .color(AMBER),
                );
            }
        } else if !self.queries.loading() {
            empty(
                ui,
                "Verify before deciding",
                "Content analysis runs locally and never chooses which copy should be removed.",
            );
        }
        if let Some(path) = reveal {
            self.reveal(&path);
        }
        self.pager(ui);
    }
    fn system(&mut self, ui: &mut egui::Ui) {
        if let Some(s) = &self.system {
            ui.horizontal_wrapped(|ui| {
                Self::metric(
                    ui,
                    "CPU",
                    format!("{:.1}%", s.cpu_percent),
                    &format!("Sample {} ms", s.sample_millis),
                );
                Self::metric(
                    ui,
                    "Memory",
                    bytes(s.used_memory),
                    &format!("of {}", bytes(s.total_memory)),
                );
                Self::metric(
                    ui,
                    "Swap",
                    bytes(s.used_swap),
                    &format!("of {}", bytes(s.total_swap)),
                );
            });
            for v in &s.volumes {
                ui.label(format!(
                    "{} · {} free of {} · {}",
                    v.mount,
                    bytes(v.available_bytes),
                    bytes(v.total_bytes),
                    v.filesystem
                ));
            }
            ui.add_space(12.0);
            ui.heading("Processes · highest memory first");
            egui::Grid::new("processes")
                .striped(true)
                .num_columns(5)
                .show(ui, |ui| {
                    for label in ["PID", "PROCESS", "CPU", "MEMORY", "I/O WRITTEN"] {
                        ui.strong(label);
                    }
                    ui.end_row();
                    for p in s.processes.iter().take(100) {
                        ui.label(p.pid.to_string());
                        ui.label(&p.name);
                        ui.label(format!("{:.1}%", p.cpu_percent));
                        ui.label(bytes(p.memory_bytes));
                        ui.label(bytes(p.disk_written_bytes));
                        ui.end_row();
                    }
                });
        }
    }
    fn history(&mut self, ui: &mut egui::Ui) {
        self.navigation(ui);
        let mut points: Vec<_> = self
            .history
            .iter()
            .filter(|p| self.path.is_empty() || p.path == self.path)
            .collect();
        if self.path.is_empty() {
            ui.label("Choose a directory above to graph comparable observations.");
        } else if points.len() >= 2 {
            points.sort_by_key(|p| p.timestamp);
            let max = points
                .iter()
                .map(|p| p.logical_bytes)
                .max()
                .unwrap_or(1)
                .max(1) as f32;
            let first = points[0].timestamp;
            let duration = (points.last().unwrap().timestamp - first).max(1) as f32;
            let (rect, _) = ui
                .allocate_exact_size(Vec2::new(ui.available_width(), 220.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 10.0, PANEL);
            let rect = rect.shrink(18.0);
            let line: Vec<_> = points
                .iter()
                .map(|p| {
                    egui::pos2(
                        rect.left() + (p.timestamp - first) as f32 / duration * rect.width(),
                        rect.bottom() - p.logical_bytes as f32 / max * rect.height(),
                    )
                })
                .collect();
            ui.painter()
                .add(egui::Shape::line(line, egui::Stroke::new(2.5, TEAL)));
            ui.label(format!(
                "Peak {} · {} observations",
                bytes(max as u64),
                points.len()
            ));
        } else {
            ui.label("At least two scans of this directory are needed to show change.");
        }
        for p in self.history.iter().take(100) {
            ui.label(format!(
                "{} · {} · {} · {}",
                p.timestamp,
                p.path,
                bytes(p.logical_bytes),
                p.coverage
            ));
        }
    }
}
impl App {
    fn render(&mut self, ctx: &egui::Context) {
        self.receive();
        ctx.request_repaint_after(std::time::Duration::from_millis(if self.busy {
            150
        } else {
            30_000
        }));
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::R)) {
            self.refresh();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) {
            self.show_scan_dialog = true;
        }
        for (key, page) in [
            (egui::Key::Num1, Page::Overview),
            (egui::Key::Num2, Page::Map),
            (egui::Key::Num3, Page::Insights),
            (egui::Key::Num4, Page::Cleanup),
        ] {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, key)) {
                self.choose_page(page);
            }
        }
        if let Some(file) = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()))
        {
            self.root = file.display().to_string();
            self.show_scan_dialog = true;
        }
        egui::SidePanel::left("navigation")
            .exact_width(205.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(18, 21, 32))
                    .inner_margin(egui::Margin::symmetric(15, 20)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 7.0;
                ui.add_space(8.0);
                ui.label(
                    RichText::new("stratum")
                        .size(30.0)
                        .strong()
                        .color(Color32::from_rgb(227, 224, 255)),
                );
                eyebrow(ui, "Local machine intelligence");
                ui.add_space(24.0);
                for (section, pages) in [
                    ("WORKSPACE", &[Page::Overview, Page::Insights][..]),
                    (
                        "STORAGE",
                        &[Page::Map, Page::Explorer, Page::Apps, Page::Duplicates][..],
                    ),
                    (
                        "REVIEW & OBSERVE",
                        &[Page::Cleanup, Page::History, Page::System, Page::Audit][..],
                    ),
                ] {
                    eyebrow(ui, section);
                    for &page in pages {
                        let chosen = self.page == page;
                        let button =
                            egui::Button::new(RichText::new(page.title()).color(if chosen {
                                Color32::WHITE
                            } else {
                                MUTED
                            }))
                            .fill(if chosen {
                                Color32::from_rgb(57, 52, 85)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .corner_radius(8);
                        if ui.add_sized([174.0, 34.0], button).clicked() {
                            self.choose_page(page);
                        }
                    }
                    ui.add_space(12.0);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new("No telemetry · no account")
                            .size(11.0)
                            .color(MUTED),
                    );
                    pill(ui, "ON THIS DEVICE", TEAL);
                });
            });
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(18, 21, 32))
                    .inner_margin(egui::Margin::symmetric(18, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy || self.queries.loading() {
                        ui.spinner();
                    }
                    ui.label(RichText::new(&self.status).small().color(MUTED));
                    if let Some(scan) = self.active_scan.clone() {
                        if ui
                            .small_button(if self.paused { "Resume" } else { "Pause" })
                            .clicked()
                        {
                            match self
                                .engine
                                .control_scan(&scan, if self.paused { "resume" } else { "pause" })
                            {
                                Ok(()) => self.paused = !self.paused,
                                Err(e) => self.error = Some(e.to_string()),
                            }
                        }
                        if ui.small_button("Cancel scan").clicked() {
                            self.engine.cancel_all();
                        }
                    } else if self.duplicate_running && ui.small_button("Cancel hashing").clicked()
                    {
                        self.duplicate_cancel
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    if self.queries.loading() {
                        ui.label(RichText::new("Reading index…").small().color(ACCENT));
                    }
                });
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14,17,26)).inner_margin(24)).show(ctx,|ui|{
            ui.horizontal(|ui|{
                ui.label(RichText::new(self.page.title()).size(30.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                    if ui.add_enabled(!self.busy,egui::Button::new("Scan a location…").fill(ACCENT.gamma_multiply(0.3))).clicked(){self.show_scan_dialog=true;}
                    if ui.button("Refresh").on_hover_text("Command/Ctrl + R").clicked(){self.refresh();}
                });
            });
            ui.label(RichText::new(self.page.subtitle()).size(13.0).color(MUTED));
            ui.add_space(18.0);
            if let Some(error)=self.error.clone(){
                egui::Frame::new().fill(Color32::from_rgb(60,37,43)).corner_radius(9).inner_margin(12).show(ui,|ui|{
                    ui.horizontal_wrapped(|ui|{ui.label(RichText::new(error).color(Color32::LIGHT_RED));if ui.small_button("Dismiss").clicked(){self.error=None;}});
                });
                ui.add_space(10.0);
            }
            if self.page==Page::Cleanup { self.cleanup_toolbar(ui); }
            egui::ScrollArea::vertical().id_salt((self.page.title(),self.plan.as_ref().map(|p|p.id.as_str()),self.operation.as_ref().map(|o|o.id.as_str()))).show(ui,|ui|match self.page{
                Page::Overview=>self.overview(ui),
                Page::Map=>self.map(ui),
                Page::Explorer=>self.explorer(ui),
                Page::Cleanup=>self.cleanup(ui),
                Page::Apps=>self.apps(ui),
                Page::Duplicates=>self.duplicates(ui),
                Page::System=>self.system(ui),
                Page::Insights=>{
                    if self.insights.is_empty() && !self.queries.loading(){empty(ui,"No findings in this scope","Scan development folders or collect more observations. No findings is not a guarantee of system health.");}
                    let insights:Vec<_>=self.insights.iter().take(100).cloned().collect();
                    for insight in &insights{self.actionable_insight(ui,insight,false);}
                    if self.insights.len()>100{ui.label("Showing the 100 largest findings. The full list is available through the API.");}
                }
                Page::History=>self.history(ui),
                Page::Audit=>{
                    if self.audit.is_empty(){empty(ui,"Your actions leave a record","Scans, plans, moves and restores will appear here.");}
                    for record in &self.audit{card().show(ui,|ui|{
                        ui.horizontal_wrapped(|ui|{ui.strong(record.action.replace('_'," "));ui.label(RichText::new(age(record.timestamp)).small().color(MUTED));});
                        ui.label(RichText::new(&record.resource_id).monospace().small());
                        egui::CollapsingHeader::new("Recorded detail").id_salt(record.id).show(ui,|ui|{ui.label(&record.detail);});
                    });}
                }
            });
        });
        if self.show_scan_dialog {
            let mut open = true;
            egui::Window::new("Choose a scan location").open(&mut open).collapsible(false).resizable(false).default_width(570.0).show(ctx,|ui|{
                ui.label(RichText::new("Start focused. Expand when you need to.").size(20.0).strong());
                ui.label("Scanning reads metadata and saves a local index. It does not authorize cleanup. Previous scan exclusions are preserved when rescanning a known root.");
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui|{
                    if ui.button("Choose folder…").clicked() && let Some(folder)=rfd::FileDialog::new().set_title("Choose a folder to index").pick_folder(){self.root=folder.display().to_string();}
                    for folder in ["Downloads","Projects"]{if ui.button(folder).clicked(){self.root=std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(folder).display().to_string();}}
                    if ui.button("Home").clicked(){self.root=std::env::var("HOME").unwrap_or_default();}
                });
                ui.add(egui::TextEdit::singleline(&mut self.root).desired_width(ui.available_width()).hint_text("Absolute folder path"));
                if !self.indexed_roots.is_empty(){egui::ComboBox::from_id_salt("rescan-root").selected_text("Previously indexed locations").show_ui(ui,|ui|{for root in &self.indexed_roots{ui.selectable_value(&mut self.root,root.clone(),short_path(root));}});}
                ui.label(RichText::new("macOS may restrict some folders. Warnings are recorded; Stratum does not request administrator privileges. Overlapping roots are rejected to prevent double counting.").small().color(MUTED));
                if ui.add_enabled(!self.busy && !self.root.trim().is_empty(),egui::Button::new("Start read-only scan").fill(ACCENT.gamma_multiply(0.35))).clicked(){self.start_scan();}
            });
            if !open {
                self.show_scan_dialog = false;
            }
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.render(ctx);
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.engine.cancel_all();
        self.duplicate_cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
fn bytes(n: u64) -> String {
    let mut value = n as f64;
    let mut unit = "B";
    for u in ["KiB", "MiB", "GiB", "TiB"] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = u;
    }
    format!("{value:.1} {unit}")
}
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    } else {
        s.into()
    }
}
fn main() -> eframe::Result {
    let options = Options::parse();
    let engine = match stratum_engine::load_config(options.config.as_deref(), options.data_dir)
        .and_then(Engine::open)
    {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 850.0])
            .with_min_inner_size([960.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Stratum",
        native,
        Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, engine, options.page)))),
    )
}
