use clap::Parser;
use eframe::egui::{self, Color32, RichText, Vec2};
use std::{
    collections::HashSet,
    sync::{Arc, atomic::AtomicBool, mpsc},
};
use stratum_domain::*;
use stratum_engine::Engine;

const TEAL: Color32 = Color32::from_rgb(72, 214, 184);
const MUTED: Color32 = Color32::from_rgb(139, 154, 177);
const PANEL: Color32 = Color32::from_rgb(22, 30, 43);
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
            Self::Map => "Area represents indexed logical bytes. Click a directory to drill down.",
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
    Overview(StorageExplanation),
    Files(domain::Page<Entry>),
    Candidates(domain::Page<CleanupCandidate>),
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
use stratum_engine::domain;
struct App {
    engine: Arc<Engine>,
    page: Page,
    tx: mpsc::Sender<Result<Payload>>,
    rx: mpsc::Receiver<Result<Payload>>,
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
    selected: HashSet<String>,
    plan: Option<CleanupPlan>,
    operation: Option<CleanupOperation>,
    approval: String,
    operation_lookup: String,
    uninstall: Option<serde_json::Value>,
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>, engine: Arc<Engine>, page: Page) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(14, 20, 30);
        visuals.window_fill = PANEL;
        visuals.override_text_color = Some(Color32::from_rgb(226, 233, 242));
        visuals.selection.bg_fill = Color32::from_rgb(32, 80, 75);
        visuals.selection.stroke.color = TEAL;
        visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1.0, Color32::from_rgb(43, 55, 72));
        cc.egui_ctx.set_visuals(visuals);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(12.0, 12.0);
        style.spacing.button_padding = Vec2::new(12.0, 8.0);
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(28.0));
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        cc.egui_ctx.set_style(style);
        let (tx, rx) = mpsc::channel();
        let events = engine.subscribe();
        let indexed_root = engine.roots().ok().and_then(|r| r.first().cloned());
        let root = indexed_root
            .clone()
            .unwrap_or_else(|| std::env::var("HOME").unwrap_or_default());
        let mut app = Self {
            engine,
            page,
            tx,
            rx,
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
            selected: HashSet::new(),
            plan: None,
            operation: None,
            approval: String::new(),
            operation_lookup: String::new(),
            uninstall: None,
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
        self.status = "Working…".into();
        let engine = self.engine.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work(engine));
        });
    }
    fn refresh(&mut self) {
        let page = self.page;
        let path = self.path.clone();
        let offset = self.offset;
        let sort = self.sort.clone();
        let search = self.search.clone();
        self.task(move |e| match page {
            Page::Overview => Ok(Payload::Overview(e.explain_storage()?)),
            Page::Explorer | Page::Map => Ok(Payload::Files(e.files(&FileQuery {
                parent: if path.is_empty() { None } else { Some(path) },
                kind: if page == Page::Map {
                    Some("directory".into())
                } else {
                    None
                },
                name: if search.is_empty() {
                    None
                } else {
                    Some(search)
                },
                sort,
                limit: 100,
                offset,
                ..Default::default()
            })?)),
            Page::Cleanup => Ok(Payload::Candidates(e.cleanup_candidates(&FileQuery {
                limit: 100,
                offset,
                ..Default::default()
            })?)),
            Page::Apps => Ok(Payload::Apps(e.applications()?)),
            Page::Duplicates => {
                if offset == 0 {
                    Ok(Payload::Duplicates(e.duplicates()?))
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
        });
    }
    fn receive(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            self.busy = false;
            self.status = "Updated · local only".into();
            match result {
                Err(e) => {
                    self.status = "Operation failed".into();
                    self.error = Some(e.to_string());
                }
                Ok(payload) => match payload {
                    Payload::Overview(v) => {
                        if self.path.is_empty()
                            && let Some(scan) = v
                                .scans
                                .iter()
                                .find(|s| s.status == "completed" || s.status == "partial")
                        {
                            self.path = scan.root.clone();
                        }
                        self.overview = Some(v);
                    }
                    Payload::Files(v) => {
                        self.files = v.items;
                        self.has_more = v.has_more;
                    }
                    Payload::Candidates(v) => {
                        self.candidates = v.items;
                        self.has_more = v.has_more;
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
                        self.plan = Some(v);
                        self.approval.clear();
                    }
                    Payload::Operation(v) => {
                        self.operation_lookup = v.id.clone();
                        self.operation = Some(v);
                        self.selected.clear();
                    }
                    Payload::Scanned(v) => {
                        self.status = format!(
                            "Indexed {} entries · {} warnings",
                            v.iter().map(|s| s.entries).sum::<u64>(),
                            v.iter().map(|s| s.warnings).sum::<u64>()
                        );
                        self.refresh();
                    }
                },
            }
        }
        while let Ok(event) = self.events.try_recv() {
            match event {
                OperationEvent::ScanProgress { entries, .. } => {
                    self.status = format!("Indexing · {entries} entries")
                }
                OperationEvent::ScanWarning { code, .. } => {
                    self.status = format!("Indexing with warning · {code}")
                }
                _ => {}
            }
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
    fn overview(&mut self, ui: &mut egui::Ui) {
        let Some(v) = &self.overview else {
            ui.label("Scan a directory to begin building your local storage index.");
            return;
        };
        let total = v.categories.iter().map(|c| c.logical_bytes).sum();
        let files = v.categories.iter().map(|c| c.files).sum::<u64>();
        let warnings = v.scans.first().map_or(0, |s| s.warnings);
        ui.horizontal_wrapped(|ui| {
            Self::metric(
                ui,
                "Indexed storage",
                bytes(total),
                "Logical bytes across indexed files",
            );
            let volume = v
                .resources
                .volumes
                .iter()
                .find(|volume| volume.mount == "/")
                .or_else(|| v.resources.volumes.first());
            Self::metric(
                ui,
                "Available on system volume",
                volume.map_or_else(|| "Unavailable".into(), |v| bytes(v.available_bytes)),
                volume.map_or("No volume observation", |v| v.mount.as_str()),
            );
            Self::metric(
                ui,
                "CPU",
                format!("{:.1}%", v.resources.cpu_percent),
                "Measured at last refresh",
            );
            Self::metric(
                ui,
                "Memory",
                bytes(v.resources.used_memory),
                &format!("of {} total", bytes(v.resources.total_memory)),
            );
        });
        ui.label(
            RichText::new(format!(
                "{files} indexed files · {} evidence-based insights · {warnings} coverage warnings",
                v.insights.len()
            ))
            .small()
            .color(MUTED),
        );
        ui.add_space(14.0);
        ui.columns(2,|cols|{
            cols[0].heading("Storage composition");for (i,c) in v.categories.iter().enumerate(){let ratio=if total==0{0.0}else{c.logical_bytes as f32/total as f32};cols[0].horizontal(|ui|{ui.label(RichText::new(c.category.replace('_'," ")).color(COLORS[i%6]));ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.label(bytes(c.logical_bytes));});});cols[0].add(egui::ProgressBar::new(ratio).fill(COLORS[i%6]).desired_height(7.0));}
            cols[1].heading("Worth a closer look");if v.insights.is_empty(){cols[1].label("No rule findings yet. Scan development folders or build a history with another scan.");}for i in v.insights.iter().take(4){insight_card(&mut cols[1],i);}
        });
        ui.add_space(16.0);
        ui.label(RichText::new(&v.interpretation).small().color(MUTED));
        egui::CollapsingHeader::new("Scan coverage & freshness").show(ui, |ui| {
            for s in &v.scans {
                ui.label(format!(
                    "{} · {} · {} · {} warnings · {} exclusions",
                    s.root, s.status, s.freshness, s.warnings, s.excluded
                ));
            }
        });
    }
    fn navigation(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Directory");
            let response = ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(400.0));
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.offset = 0;
                self.refresh();
            }
            if ui.button("Open").clicked() {
                self.offset = 0;
                self.refresh();
            }
            if ui.button("Parent").clicked()
                && let Some(parent) = std::path::Path::new(&self.path).parent()
            {
                self.path = parent.to_string_lossy().into();
                self.offset = 0;
                self.refresh();
            }
        });
    }
    fn explorer(&mut self, ui: &mut egui::Ui) {
        self.navigation(ui);
        ui.horizontal(|ui| {
            ui.label("Name glob");
            ui.text_edit_singleline(&mut self.search);
            egui::ComboBox::from_id_salt("sort")
                .selected_text(&self.sort)
                .show_ui(ui, |ui| {
                    for sort in ["logical_bytes", "allocated_bytes", "modified_at", "path"] {
                        ui.selectable_value(&mut self.sort, sort.into(), sort);
                    }
                });
            if ui.button("Apply").clicked() {
                self.offset = 0;
                self.refresh();
            }
        });
        let mut navigate = None;
        egui::Grid::new("files")
            .num_columns(4)
            .striped(true)
            .spacing([22.0, 12.0])
            .min_col_width(90.0)
            .show(ui, |ui| {
                ui.strong("NAME / PATH");
                ui.strong("LOGICAL");
                ui.strong("ALLOCATED");
                ui.strong("CATEGORY");
                ui.end_row();
                for e in &self.files {
                    if e.kind == EntryKind::Directory {
                        if ui
                            .link(format!("{} /", e.name))
                            .on_hover_text(&e.path)
                            .clicked()
                        {
                            navigate = Some(e.path.clone());
                        }
                    } else {
                        ui.label(&e.name).on_hover_text(&e.path);
                    }
                    ui.label(bytes(e.logical_bytes));
                    ui.label(bytes(e.allocated_bytes));
                    ui.label(e.category.replace('_', " "))
                        .on_hover_text(format!(
                            "confidence {:.0}% · {}",
                            e.confidence * 100.0,
                            e.evidence.first().map(|e| e.detail.as_str()).unwrap_or("")
                        ));
                    ui.end_row();
                }
            });
        if let Some(path) = navigate {
            self.path = path;
            self.offset = 0;
            self.refresh();
        }
        self.pager(ui);
    }
    fn pager(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.offset >= 100, egui::Button::new("Previous"))
                .clicked()
            {
                self.offset -= 100;
                self.refresh();
            }
            ui.label(format!("Offset {} · page size 100", self.offset));
            if ui
                .add_enabled(self.has_more, egui::Button::new("Next"))
                .clicked()
            {
                self.offset += 100;
                self.refresh();
            }
        });
    }
    fn map(&mut self, ui: &mut egui::Ui) {
        self.navigation(ui);
        if self.files.is_empty() {
            ui.label("No child directories in this indexed location.");
            return;
        }
        let items: Vec<_> = self.files.iter().filter(|e| e.logical_bytes > 0).collect();
        let total = items.iter().map(|e| e.logical_bytes).sum::<u64>();
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 420.0), egui::Sense::hover());
        let mut tiles = vec![];
        treemap(&items, rect, &mut tiles);
        let mut selected = None;
        for (i, (e, rect)) in tiles.into_iter().enumerate() {
            let color = COLORS[i % 6].gamma_multiply(0.5);
            ui.painter().rect_filled(rect.shrink(2.0), 6.0, color);
            let r = ui.interact(rect, ui.id().with(&e.path), egui::Sense::click());
            if rect.width() > 75.0 && rect.height() > 45.0 {
                ui.painter().text(
                    rect.min + Vec2::splat(12.0),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "{}\n{}",
                        truncate(&e.name, ((rect.width() - 24.0) / 8.0) as usize),
                        bytes(e.logical_bytes)
                    ),
                    egui::FontId::proportional(14.0),
                    Color32::WHITE,
                );
            }
            if r.on_hover_text(format!("{}\n{}", e.path, bytes(e.logical_bytes)))
                .clicked()
            {
                selected = Some(e.path.clone());
            }
        }
        ui.label(RichText::new(format!("{} across {} displayed child directories. Direct files and additional pages are not shown.",bytes(total),items.len())).color(MUTED));
        if let Some(path) = selected {
            self.path = path;
            self.offset = 0;
            self.refresh();
        }
    }
    fn cleanup(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Existing operation ID");
            ui.text_edit_singleline(&mut self.operation_lookup);
            if ui
                .add_enabled(
                    !self.busy && !self.operation_lookup.is_empty(),
                    egui::Button::new("Load for inspection / restore"),
                )
                .clicked()
            {
                let id = self.operation_lookup.clone();
                self.task(move |e| Ok(Payload::Operation(e.cleanup_operation(&id)?)));
            }
        });
        ui.label(RichText::new("Quarantine moves files on the same filesystem. It preserves undo but does not free disk space until the files are removed later.").color(Color32::from_rgb(231,180,99)));
        ui.label("Candidate discovery is conservative: verified Cargo target files and recognized package caches. Stop active builds before applying a plan.");
        if ui
            .add_enabled(
                !self.selected.is_empty() && !self.busy,
                egui::Button::new(format!(
                    "Create plan for {} selected files",
                    self.selected.len()
                )),
            )
            .clicked()
        {
            let paths = self.selected.iter().cloned().collect();
            self.task(move |e| Ok(Payload::Plan(e.create_cleanup_plan(PlanRequest { paths })?)));
        }
        for c in &self.candidates {
            ui.horizontal(|ui| {
                let mut checked = self.selected.contains(&c.path);
                if ui.checkbox(&mut checked, "").changed() {
                    if checked {
                        self.selected.insert(c.path.clone());
                    } else {
                        self.selected.remove(&c.path);
                    }
                }
                ui.label(RichText::new(bytes(c.size)).color(TEAL));
                ui.label(&c.path);
            });
            ui.label(
                RichText::new(format!("{} · risk {} · reversible", c.reason, c.risk))
                    .small()
                    .color(MUTED),
            );
        }
        self.pager(ui);
        if let Some(p) = self.plan.clone() {
            ui.separator();
            ui.heading("Review immutable plan");
            ui.label(format!(
                "{} · {} files · {} · {} risk",
                p.id,
                p.items.len(),
                bytes(p.total_bytes),
                p.risk
            ));
            egui::CollapsingHeader::new("Exact approved selection")
                .default_open(true)
                .show(ui, |ui| {
                    for item in &p.items {
                        ui.label(format!("{}  {}", bytes(item.bytes), item.path));
                    }
                });
            ui.label("Type this exact phrase to authorize the move:");
            ui.monospace(&p.approval_phrase);
            ui.text_edit_singleline(&mut self.approval);
            if ui
                .add_enabled(
                    self.approval == p.approval_phrase && !self.busy,
                    egui::Button::new("Approve and quarantine")
                        .fill(Color32::from_rgb(116, 64, 44)),
                )
                .clicked()
            {
                let approval = self.approval.clone();
                self.task(move |e| {
                    Ok(Payload::Operation(
                        e.execute_cleanup_plan(&p.id, &approval)?,
                    ))
                });
            }
        }
        if let Some(o) = self.operation.clone() {
            ui.separator();
            ui.heading(format!("Operation · {}", o.status));
            ui.monospace(&o.id);
            for item in &o.items {
                ui.label(format!("{} · {}", item.status, item.source));
                if let Some(error) = &item.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Restore quarantined files"))
                .clicked()
            {
                self.task(move |e| Ok(Payload::Operation(e.undo_cleanup(&o.id)?)));
            }
        }
    }
    fn apps(&mut self, ui: &mut egui::Ui) {
        if self.apps.is_empty() {
            ui.label(
                "No indexed macOS bundles. Scan /Applications and relevant user Library locations.",
            );
        }
        let mut requested = None;
        for app in &self.apps {
            egui::CollapsingHeader::new(format!("{}     {}",app.name,bytes(app.footprint_bytes))).show(ui,|ui|{ui.label(&app.coverage);for a in &app.associations{ui.label(format!("{} · {} · {}",bytes(a.bytes),a.confidence,a.path));for e in &a.evidence{ui.label(RichText::new(&e.detail).small().color(MUTED));}}ui.label("Uninstall execution is unsupported in this release; associations are for review.");});
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(format!("Prepare uninstall review for {}", app.name)),
                )
                .clicked()
            {
                requested = Some(app.id.clone());
            }
        }
        if let Some(id) = requested {
            self.task(move |e| Ok(Payload::Uninstall(e.uninstall_plan(&id)?)));
        }
        if let Some(review) = &self.uninstall {
            egui::CollapsingHeader::new("Uninstall review proposal · no filesystem changes")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(serde_json::to_string_pretty(review).unwrap_or_default());
                });
        }
    }
    fn duplicates(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(!self.busy, egui::Button::new("Analyze duplicate content"))
            .clicked()
        {
            self.offset = 0;
            self.task(|e| {
                Ok(Payload::Duplicates(
                    e.discover_duplicates(&AtomicBool::new(false))?,
                ))
            });
        }
        if let Some(report) = &self.duplicates {
            ui.label(format!(
                "{} total groups · {} files fully hashed in this run · {} warnings",
                report.group_count,
                report.files_hashed,
                report.warnings.len()
            ));
            for g in &report.groups {
                egui::CollapsingHeader::new(format!(
                    "{} copies · {} each · {} potentially redundant",
                    g.file_count,
                    bytes(g.file_size),
                    bytes(g.reclaimable_size)
                ))
                .show(ui, |ui| {
                    for file in &g.files {
                        ui.label(file);
                    }
                    ui.label(RichText::new(&g.verification).small().color(MUTED));
                });
            }
            for w in &report.warnings {
                ui.label(format!("{}: {}", w.mechanism, w.detail));
            }
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
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.receive();
        ctx.request_repaint_after(std::time::Duration::from_millis(if self.busy {
            150
        } else {
            2000
        }));
        egui::SidePanel::left("navigation")
            .exact_width(205.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(24.0);
                ui.label(RichText::new("STRATUM").size(23.0).strong().color(TEAL));
                ui.label(RichText::new("MACHINE INTELLIGENCE").small().color(MUTED));
                ui.add_space(30.0);
                for page in [
                    Page::Overview,
                    Page::Explorer,
                    Page::Map,
                    Page::Cleanup,
                    Page::Apps,
                    Page::Duplicates,
                    Page::System,
                    Page::Insights,
                    Page::History,
                    Page::Audit,
                ] {
                    let selected = self.page == page;
                    let button =
                        egui::Button::new(RichText::new(page.title()).color(if selected {
                            TEAL
                        } else {
                            Color32::from_rgb(184, 197, 215)
                        }))
                        .fill(if selected {
                            Color32::from_rgb(30, 55, 58)
                        } else {
                            Color32::TRANSPARENT
                        });
                    if ui.add_sized([180.0, 38.0], button).clicked() && !self.busy {
                        self.page = page;
                        self.offset = 0;
                        self.refresh();
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(RichText::new("LOCAL ONLY").color(TEAL).small());
                    ui.label(RichText::new("0.1 · No telemetry").small().color(MUTED));
                });
            });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.busy {
                    ui.spinner();
                }
                ui.label(RichText::new(&self.status).small().color(MUTED));
                if self.busy && ui.small_button("Cancel scan").clicked() {
                    self.engine.cancel_all();
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.heading(self.page.title());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("↻ Refresh"))
                        .clicked()
                    {
                        self.refresh();
                    }
                });
            });
            ui.label(RichText::new(self.page.subtitle()).color(MUTED));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label("Scan root");
                ui.add(egui::TextEdit::singleline(&mut self.root).desired_width(360.0));
                if ui
                    .add_enabled(
                        !self.busy && !self.root.is_empty(),
                        egui::Button::new("Index directory").fill(Color32::from_rgb(27, 86, 75)),
                    )
                    .clicked()
                {
                    let root = self.root.clone();
                    self.task(move |e| {
                        Ok(Payload::Scanned(e.scan(ScanRequest {
                            roots: vec![root],
                            ..Default::default()
                        })?))
                    });
                }
            });
            ui.separator();
            if let Some(error) = &self.error {
                egui::Frame::new()
                    .fill(Color32::from_rgb(67, 35, 42))
                    .corner_radius(8)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.label(RichText::new(error).color(Color32::LIGHT_RED));
                    });
            }
            egui::ScrollArea::vertical().show(ui, |ui| match self.page {
                Page::Overview => self.overview(ui),
                Page::Explorer => self.explorer(ui),
                Page::Map => self.map(ui),
                Page::Cleanup => self.cleanup(ui),
                Page::Apps => self.apps(ui),
                Page::Duplicates => self.duplicates(ui),
                Page::System => self.system(ui),
                Page::Insights => {
                    if self.insights.is_empty() {
                        ui.label("No rule findings for the current index.");
                    }
                    for insight in &self.insights {
                        insight_card(ui, insight);
                    }
                }
                Page::History => self.history(ui),
                Page::Audit => {
                    for a in &self.audit {
                        ui.label(format!(
                            "{} · {} · {}",
                            a.timestamp, a.action, a.resource_id
                        ));
                        ui.label(RichText::new(&a.detail).small().color(MUTED));
                        ui.separator();
                    }
                }
            });
        });
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
fn insight_card(ui: &mut egui::Ui, i: &Insight) {
    egui::Frame::new()
        .fill(PANEL)
        .corner_radius(10)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.label(RichText::new(&i.title).strong());
            ui.label(&i.description);
            ui.label(
                RichText::new(format!(
                    "{} impact · {:.0}% confidence · {} risk",
                    bytes(i.estimated_impact),
                    i.confidence * 100.0,
                    i.risk
                ))
                .small()
                .color(TEAL),
            );
            egui::CollapsingHeader::new("Evidence")
                .id_salt(&i.id)
                .show(ui, |ui| {
                    for e in &i.evidence {
                        ui.label(format!("{}: {}", e.mechanism, e.detail));
                    }
                });
        });
}
fn treemap<'a>(items: &[&'a Entry], rect: egui::Rect, out: &mut Vec<(&'a Entry, egui::Rect)>) {
    if items.is_empty() {
        return;
    }
    if items.len() == 1 {
        out.push((items[0], rect));
        return;
    }
    let total = items.iter().map(|e| e.logical_bytes).sum::<u64>();
    if total == 0 {
        return;
    }
    let mut sum = 0;
    let mut split = 1;
    for (i, e) in items.iter().enumerate().take(items.len() - 1) {
        sum += e.logical_bytes;
        split = i + 1;
        if sum >= total / 2 {
            break;
        }
    }
    let ratio = sum as f32 / total as f32;
    let (mut a, mut b) = (rect, rect);
    if rect.width() > rect.height() {
        a.max.x = rect.min.x + rect.width() * ratio;
        b.min.x = a.max.x;
    } else {
        a.max.y = rect.min.y + rect.height() * ratio;
        b.min.y = a.max.y;
    }
    treemap(&items[..split], a, out);
    treemap(&items[split..], b, out);
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
        Box::new(move |cc| Ok(Box::new(App::new(cc, engine, options.page)))),
    )
}
