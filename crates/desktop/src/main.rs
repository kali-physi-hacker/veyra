mod apps;
mod audit;
mod brand;
mod cleanup;
mod duplicates;
mod fonts;
mod history;
mod icons;
mod insights;
mod kit;
mod overview;
mod shell;
mod storage;
mod system;
mod theme;
#[cfg(test)]
mod ui_tests;
mod util;
mod work;

use clap::Parser;
use eframe::egui::{self, Color32};
use fonts::Weight;
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::Instant,
};
use stratum_domain::*;
use stratum_engine::{Engine, domain};
use theme::Palette;
use util::*;

#[derive(Parser)]
struct Options {
    #[arg(long, env = "STRATUM_DATA_DIR")]
    data_dir: Option<std::path::PathBuf>,
    #[arg(long)]
    config: Option<std::path::PathBuf>,
    #[arg(long, value_enum, default_value = "overview")]
    page: Page,
    /// Force a colour scheme instead of following the system appearance.
    #[arg(long, value_enum)]
    appearance: Option<Appearance>,
    /// Open the scan-location dialog immediately (nothing scans until confirmed).
    #[arg(long, hide = true)]
    open_scan_dialog: bool,
}
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Appearance {
    Dark,
    Light,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
enum Page {
    Overview,
    Insights,
    Map,
    Explorer,
    Apps,
    Duplicates,
    Cleanup,
    History,
    System,
    Audit,
}
impl Page {
    fn sections() -> [(&'static str, &'static [Page]); 3] {
        [
            ("Workspace", &[Page::Overview, Page::Insights]),
            (
                "Storage",
                &[Page::Map, Page::Explorer, Page::Apps, Page::Duplicates],
            ),
            (
                "Review",
                &[Page::Cleanup, Page::History, Page::System, Page::Audit],
            ),
        ]
    }
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
            Self::Overview => "Your machine, explained. Everything stays on this device.",
            Self::Explorer => "Browse indexed paths. Open a folder to see its immediate children.",
            Self::Map => {
                "Files and folders in proportion. Inspect an item or open a folder to go deeper."
            }
            Self::Cleanup => {
                "Select exact files, review an immutable plan, then approve explicitly."
            }
            Self::Apps => "Estimated footprints with evidence for every associated location.",
            Self::Duplicates => {
                "Size, then sampled fingerprint, then full BLAKE3. You decide which copies matter."
            }
            Self::System => "A measured resource snapshot. Refresh to sample again.",
            Self::Insights => "Deterministic findings with evidence, confidence, impact and risk.",
            Self::History => {
                "Observed directory totals over time. Missing periods are never inferred."
            }
            Self::Audit => {
                "A durable local record of indexing, planning, quarantine and restoration."
            }
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Overview => icons::HOUSE,
            Self::Insights => icons::LIGHTBULB,
            Self::Map => icons::SQUARES_FOUR,
            Self::Explorer => icons::FOLDER_OPEN,
            Self::Apps => icons::APP_WINDOW,
            Self::Duplicates => icons::COPY,
            Self::Cleanup => icons::BROOM,
            Self::History => icons::CHART_LINE_UP,
            Self::System => icons::CPU,
            Self::Audit => icons::CLIPBOARD_TEXT,
        }
    }
    fn color(self, p: &Palette) -> Color32 {
        match self {
            Self::Overview => p.accent,
            Self::Insights => p.amber,
            Self::Map => p.teal,
            Self::Explorer => p.blue,
            Self::Apps => p.rose,
            Self::Duplicates => p.purple,
            Self::Cleanup => p.orange,
            Self::History => p.cyan,
            Self::System => p.green,
            Self::Audit => p.text_2,
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
/// What the activity card at the bottom right is describing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Activity {
    Scan,
    Duplicates,
    Cleanup,
    Generic,
}
struct App {
    engine: Arc<Engine>,
    page: Page,
    tx: mpsc::Sender<Message>,
    rx: mpsc::Receiver<Message>,
    ctx: egui::Context,
    queries: work::LatestRequest,
    events: tokio::sync::broadcast::Receiver<OperationEvent>,
    palette: Palette,
    page_entered: Instant,
    reveal: f32,
    busy: bool,
    activity: Activity,
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
    scan_started: Option<Instant>,
    scan_entries: u64,
    scan_bytes: u64,
    /// The running scan replaces a saved index, which stays visible until it finishes.
    rescanning: bool,
    last_live_refresh: Instant,
    paused: bool,
    duplicate_cancel: Arc<AtomicBool>,
    duplicate_running: bool,
    process_sort: system::ProcessSort,
    expanded_audit: Option<i64>,
}
impl App {
    #[cfg(test)]
    fn new(ctx: &egui::Context, engine: Arc<Engine>, page: Page) -> Self {
        Self::with_appearance(ctx, engine, page, None)
    }
    fn with_appearance(
        ctx: &egui::Context,
        engine: Arc<Engine>,
        page: Page,
        appearance: Option<Appearance>,
    ) -> Self {
        fonts::install(ctx);
        let dark = match appearance {
            Some(Appearance::Dark) => true,
            Some(Appearance::Light) => false,
            None => ctx.system_theme() != Some(egui::Theme::Light),
        };
        let palette = Palette::for_mode(dark);
        palette.apply(ctx);
        kit::set_palette(ctx, palette);
        let (tx, rx) = mpsc::channel();
        let events = engine.subscribe();
        // Pages read each root's visible generation, so a first scan fills them as it runs.
        engine.set_live_view(true);
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
            palette,
            page_entered: Instant::now(),
            reveal: 1.0,
            busy: false,
            activity: Activity::Generic,
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
            scan_entries: 0,
            scan_bytes: 0,
            rescanning: false,
            last_live_refresh: Instant::now(),
            paused: false,
            duplicate_cancel: Arc::new(AtomicBool::new(false)),
            duplicate_running: false,
            process_sort: system::ProcessSort::Memory,
            expanded_audit: None,
        };
        app.refresh();
        app
    }
    fn set_dark(&mut self, dark: bool) {
        if self.palette.dark == dark {
            return;
        }
        self.palette = Palette::for_mode(dark);
        self.palette.apply(&self.ctx);
        kit::set_palette(&self.ctx, self.palette);
    }
    fn task(
        &mut self,
        activity: Activity,
        work: impl FnOnce(Arc<Engine>) -> Result<Payload> + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.activity = activity;
        self.error = None;
        self.status = match activity {
            Activity::Scan => "Preparing read-only scan…".into(),
            Activity::Duplicates => "Hashing candidate files locally…".into(),
            Activity::Cleanup => "Validating files before any move…".into(),
            Activity::Generic => "Working locally…".into(),
        };
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
                Ok(OperationEvent::ScanStarted { scan_id, root }) => {
                    self.active_scan = Some(scan_id);
                    self.scan_entries = 0;
                    self.scan_bytes = 0;
                    self.rescanning = self.indexed_roots.contains(&root);
                    if !self.rescanning {
                        self.indexed_roots.push(root.clone());
                        if self.path.is_empty() {
                            self.path = root;
                        }
                    }
                    self.last_live_refresh = Instant::now();
                    self.refresh();
                }
                Ok(OperationEvent::ScanProgress {
                    scan_id,
                    entries,
                    bytes: count,
                    ..
                }) => {
                    self.active_scan = Some(scan_id);
                    self.scan_entries = entries;
                    self.scan_bytes = count;
                    self.status = format!(
                        "{} entries · {} discovered · {} elapsed",
                        util::count(entries),
                        bytes(count),
                        duration(
                            self.scan_started
                                .map_or(0, |t| t.elapsed().as_secs() as i64)
                        )
                    );
                    // A first scan is readable as it runs; keep the open page current.
                    if !self.rescanning
                        && self.last_live_refresh.elapsed() >= std::time::Duration::from_millis(900)
                    {
                        self.last_live_refresh = Instant::now();
                        self.refresh();
                    }
                }
                Ok(OperationEvent::ScanCompleted { scan }) => {
                    self.active_scan = None;
                    self.paused = false;
                    // A first scan that did not publish leaves nothing to browse.
                    if !self.rescanning && !matches!(scan.status.as_str(), "completed" | "partial")
                    {
                        self.indexed_roots.retain(|r| *r != scan.root);
                        if self.path == scan.root {
                            self.path.clear();
                            self.breakdown = None;
                            self.files.clear();
                        }
                    }
                    self.status = format!(
                        "Scan {} · {} entries · {} warnings",
                        scan.status,
                        util::count(scan.entries),
                        scan.warnings
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
                    self.rescanning = false;
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
        self.page_entered = Instant::now();
        self.offset = 0;
        self.has_more = false;
        self.error = None;
        self.refresh();
    }
    /// Browse a saved index without scanning it again.
    fn open_indexed(&mut self, root: String) {
        self.show_scan_dialog = false;
        self.open_location(Page::Map, root);
    }
    fn start_scan(&mut self) {
        if self.busy || self.root.trim().is_empty() {
            return;
        }
        self.show_scan_dialog = false;
        self.scan_started = Some(Instant::now());
        self.scan_entries = 0;
        self.scan_bytes = 0;
        self.paused = false;
        let root = self.root.clone();
        self.task(Activity::Scan, move |e| {
            Ok(Payload::Scanned(e.scan_location(&root)?))
        });
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
    fn render(&mut self, ctx: &egui::Context) {
        self.receive();
        ctx.request_repaint_after(std::time::Duration::from_millis(if self.busy {
            120
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
        self.reveal = kit::entrance(ctx, self.page_entered, 0.42);
        self.sidebar(ctx);
        self.content(ctx);
        self.scan_modal(ctx);
        self.activity_card(ctx);
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
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Stratum")
        .with_app_id("local.stratum.desktop")
        .with_inner_size([1280.0, 860.0])
        .with_min_inner_size([980.0, 700.0])
        .with_icon(brand::icon_data(256));
    if cfg!(target_os = "macos") {
        viewport = viewport
            .with_fullsize_content_view(true)
            .with_title_shown(false)
            .with_titlebar_shown(false);
    }
    let native = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let appearance = options.appearance;
    let page = options.page;
    let open_scan_dialog = options.open_scan_dialog;
    eframe::run_native(
        "Stratum",
        native,
        Box::new(move |cc| {
            let mut app = App::with_appearance(&cc.egui_ctx, engine, page, appearance);
            app.show_scan_dialog = open_scan_dialog;
            Ok(Box::new(app))
        }),
    )
}
