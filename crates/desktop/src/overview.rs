use super::*;
impl App {
    pub(super) fn overview(&mut self, ui: &mut egui::Ui) {
        let Some(value) = &self.overview else {
            empty(
                ui,
                "Reading your local workspace",
                "Loading saved observations. No scan or cleanup starts automatically.",
            );
            return;
        };
        let resources = value.resources.clone();
        let categories = value.categories.clone();
        let coverage = value.coverage.clone();
        let findings: Vec<_> = value.insights.iter().take(3).cloned().collect();
        let total = categories.iter().map(|c| c.logical_bytes).sum::<u64>();
        let count = categories.iter().map(|c| c.files).sum::<u64>();
        let warnings = coverage.iter().map(|s| s.warnings).sum::<u64>();
        if coverage.is_empty() {
            card().show(ui,|ui|{
                ui.set_width(ui.available_width());
                eyebrow(ui,"A clearer picture starts with one folder");
                ui.add_space(8.0);
                ui.label(RichText::new("Meet your storage.\nUnderstand what matters.").size(35.0).strong());
                ui.add_space(12.0);
                ui.label("Choose a folder to build your private, reusable index. Explore sizes, recognize development artifacts, and start a history of what changes.");
                ui.add_space(14.0);
                ui.horizontal(|ui|{
                    if ui.add(egui::Button::new("Choose a folder…").fill(ACCENT.gamma_multiply(0.35))).clicked() { self.show_scan_dialog=true; }
                    if ui.button("Start with Downloads").clicked() { self.root=std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Downloads").display().to_string();self.show_scan_dialog=true; }
                });
                ui.add_space(12.0);
                ui.label(RichText::new("Read-only discovery. No account. No cloud upload. Nothing is selected for cleanup.").color(TEAL));
            });
            ui.add_space(14.0);
            ui.columns(3, |cols| {
                for (ui, (title, detail)) in cols.iter_mut().zip([
                    (
                        "01 / Observe",
                        "Scan one folder or a volume. Permission gaps stay visible.",
                    ),
                    (
                        "02 / Understand",
                        "Explore the map and evidence behind each finding.",
                    ),
                    (
                        "03 / Decide",
                        "Review exact files before any supported action.",
                    ),
                ]) {
                    card().show(ui, |ui| {
                        ui.strong(title);
                        ui.label(RichText::new(detail).color(MUTED));
                    });
                }
            });
            return;
        }
        ui.horizontal_wrapped(|ui| {
            eyebrow(ui, "Indexed scope");
            for scan in coverage.iter().take(2) {
                if ui
                    .small_button(truncate(&short_path(&scan.root), 48))
                    .on_hover_text(&scan.root)
                    .clicked()
                {
                    self.page = Page::Map;
                    self.browse(scan.root.clone());
                }
                pill(
                    ui,
                    &scan.freshness.replace('_', " "),
                    if scan.freshness == "stale" {
                        AMBER
                    } else {
                        TEAL
                    },
                );
            }
            if coverage.len() > 2 {
                ui.label(format!("+ {} more roots", coverage.len() - 2));
            }
        });
        ui.add_space(10.0);
        let volume = resources
            .volumes
            .iter()
            .find(|v| v.mount == "/")
            .or_else(|| resources.volumes.first());
        card().show(ui,|ui|{
            ui.set_width(ui.available_width());
            ui.horizontal(|ui|{
                if let Some(volume)=volume {ring(ui,1.0-volume.available_bytes as f32/volume.total_bytes.max(1) as f32,&bytes(volume.available_bytes));}
                ui.add_space(14.0);
                ui.vertical(|ui|{
                    ui.set_width(ui.available_width());
                    eyebrow(ui,"Your machine, in perspective");
                    ui.label(RichText::new("Know where your space goes.").size(29.0).strong());
                    if let Some(volume)=volume {
                        ui.label(format!("{} used of {} on {}",bytes(volume.total_bytes.saturating_sub(volume.available_bytes)),bytes(volume.total_bytes),volume.name));
                    }
                    ui.label(RichText::new("Volume capacity is an OS measurement. Indexed logical bytes below are a different measurement, not a complete physical-disk accounting.").color(MUTED));
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui|{
                        if ui.button("Explore storage").clicked(){self.choose_page(Page::Map);}
                        if ui.button("Review findings").clicked(){self.choose_page(Page::Insights);}
                        pill(ui,"Private by design",TEAL);
                    });
                });
            });
        });
        ui.add_space(14.0);
        ui.columns(3, |cols| {
            for (ui, (label, value, detail)) in cols.iter_mut().zip([
                (
                    "Indexed storage",
                    bytes(total),
                    format!("{count} files · {} roots", coverage.len()),
                ),
                (
                    "Coverage",
                    if warnings == 0 {
                        "No recorded warnings".into()
                    } else {
                        format!("{warnings} warnings")
                    },
                    "Only selected roots are indexed".into(),
                ),
                (
                    "Resource snapshot",
                    format!("{:.0}% CPU", resources.cpu_percent),
                    format!(
                        "{} memory used · {}",
                        bytes(resources.used_memory),
                        age(resources.timestamp)
                    ),
                ),
            ]) {
                card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    eyebrow(ui, label);
                    ui.label(RichText::new(value).size(22.0).strong());
                    ui.label(RichText::new(detail).small().color(MUTED));
                });
            }
        });
        ui.add_space(20.0);
        ui.columns(2,|cols|{
            cols[0].heading("What occupies the index");
            cols[0].label(RichText::new("Logical sizes · categories can be heuristic").small().color(MUTED));
            card().show(&mut cols[0],|ui|{
                ui.set_width(ui.available_width());
                for (i,category) in categories.iter().take(8).enumerate(){
                    ui.horizontal(|ui|{ui.label(category.category.replace('_'," "));ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.label(RichText::new(bytes(category.logical_bytes)).strong());});});
                    ui.add(egui::ProgressBar::new(category.logical_bytes as f32/total.max(1) as f32).fill(COLORS[i%6]).desired_height(5.0));
                    ui.add_space(6.0);
                }
            });
            cols[1].heading("Worth your attention");
            cols[1].label(RichText::new("Observed facts, with a path to investigate").small().color(MUTED));
            if findings.is_empty(){empty(&mut cols[1],"A baseline, not a verdict","No rules produced findings in this scope. Rescan later to observe changes; absence of findings is not a system health assessment.");}
            for insight in &findings {self.actionable_insight(&mut cols[1],insight,true);}
        });
        ui.add_space(20.0);
        ui.heading("Your indexed locations");
        for scan in coverage {
            card().show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(short_path(&scan.root)).on_hover_text(&scan.root);
                    pill(
                        ui,
                        &scan.freshness.replace('_', " "),
                        if scan.freshness == "stale" {
                            AMBER
                        } else {
                            TEAL
                        },
                    );
                    ui.label(format!(
                        "{} · {} warnings · {} excluded",
                        bytes(scan.logical_bytes),
                        scan.warnings,
                        scan.excluded
                    ));
                    if ui.small_button("Explore").clicked() {
                        self.page = Page::Map;
                        self.browse(scan.root.clone());
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Rescan").small())
                        .clicked()
                    {
                        self.root = scan.root.clone();
                        self.start_scan();
                    }
                });
                ui.label(
                    RichText::new(format!(
                        "{} · last full scan {}",
                        scan.status,
                        scan.completed_at.map_or_else(|| "unknown".into(), age)
                    ))
                    .small()
                    .color(MUTED),
                );
            });
        }
    }
    pub(super) fn actionable_insight(
        &mut self,
        ui: &mut egui::Ui,
        insight: &Insight,
        compact: bool,
    ) {
        card().show(ui,|ui|{
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui|{eyebrow(ui,&insight.kind.replace('_'," "));pill(ui,&bytes(insight.estimated_impact),if insight.severity=="warning"{AMBER}else{ACCENT});});
            ui.label(RichText::new(&insight.title).size(17.0).strong());
            if !compact {ui.label(&insight.description);}
            if let Some(path)=insight.related_resources.first(){ui.label(RichText::new(truncate(&short_path(path),70)).small().color(MUTED)).on_hover_text(path);}
            ui.horizontal_wrapped(|ui|{
                if ui.small_button("Investigate").clicked() && let Some(path)=insight.related_resources.first(){
                    let target=if insight.possible_actions.iter().any(|a|a=="inspect_file") {std::path::Path::new(path).parent().map_or_else(||path.clone(),|p|p.display().to_string())}else{path.clone()};
                    self.page=Page::Explorer;self.browse(target);
                }
                if insight.possible_actions.iter().any(|a|a=="create_cleanup_plan") && ui.small_button("Review candidates").clicked(){
                    self.cleanup_scope=insight.related_resources.first().cloned();self.selected.clear();self.plan=None;self.operation=None;self.choose_page(Page::Cleanup);
                }
                ui.label(RichText::new(format!("{:.0}% confidence · {} risk",insight.confidence*100.0,insight.risk)).size(11.0).color(MUTED));
            });
            egui::CollapsingHeader::new("Why this finding?").id_salt(&insight.id).show(ui,|ui|{
                if compact {ui.label(&insight.description);}
                for e in &insight.evidence {ui.label(RichText::new(&e.mechanism).small().color(ACCENT));ui.label(&e.detail);}
                ui.label(RichText::new("Impact values can overlap. Do not add them together as reclaimable space.").small().color(AMBER));
            });
        });
        ui.add_space(4.0);
    }
}
