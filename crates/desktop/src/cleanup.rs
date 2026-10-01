use super::*;
use eframe::egui::{Align, Layout, Sense, vec2};
use kit::{Button, Card, Row};

impl App {
    pub(super) fn cleanup_toolbar(&mut self, ui: &mut egui::Ui) {
        if self.plan.is_some() || self.operation.is_some() {
            return;
        }
        let p = self.palette;
        let selected_bytes = self.selected.values().copied().sum::<u64>();
        let count = self.selected.len();
        let mut card = Card::new().padding(12.0).radius(14.0);
        if count > 0 {
            card = card.tinted(p.accent);
        }
        card.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(36.0);
                // The buttons are laid out first, from the right, so the summary on the left fits
                // the space they leave instead of running under them in a narrow window.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if Button::primary("Create review plan")
                        .icon(icons::LIST_CHECKS)
                        .enabled(!self.busy && count > 0 && count <= 1000)
                        .show(ui)
                        .clicked()
                    {
                        let paths = self.selected.keys().cloned().collect();
                        self.task(Activity::Cleanup, move |e| {
                            Ok(Payload::Plan(e.create_cleanup_plan(PlanRequest { paths })?))
                        });
                    }
                    if Button::ghost("Clear")
                        .small()
                        .enabled(!self.busy && count > 0)
                        .show(ui)
                        .clicked()
                    {
                        self.selected.clear();
                    }
                    if Button::ghost("Select this page")
                        .small()
                        .enabled(!self.busy && !self.candidates.is_empty())
                        .show(ui)
                        .clicked()
                    {
                        for candidate in &self.candidates {
                            if self.selected.len() < 1000 {
                                self.selected.insert(candidate.path.clone(), candidate.size);
                            }
                        }
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        kit::icon_tile(
                            ui,
                            if count > 0 {
                                icons::CHECK_SQUARE
                            } else {
                                icons::SQUARE
                            },
                            if count > 0 { p.accent } else { p.text_3 },
                            34.0,
                        );
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            let width = ui.available_width();
                            for (text, size, weight, color) in [
                                (
                                    format!("{count} selected · {}", bytes(selected_bytes)),
                                    14.5,
                                    Weight::SemiBold,
                                    p.text,
                                ),
                                (
                                    "Quarantine first; deleting permanently is a separate step."
                                        .to_string(),
                                    12.0,
                                    Weight::Regular,
                                    p.text_3,
                                ),
                            ] {
                                let galley = kit::galley_truncated(
                                    ui,
                                    &text,
                                    fonts::font(size, weight),
                                    color,
                                    width,
                                );
                                let (rect, response) =
                                    ui.allocate_exact_size(galley.size(), Sense::hover());
                                let clipped = galley.size().x + 1.0
                                    < ui.fonts_mut(|f| {
                                        f.layout_no_wrap(
                                            text.clone(),
                                            fonts::font(size, weight),
                                            color,
                                        )
                                        .size()
                                        .x
                                    });
                                ui.painter().galley(rect.min, galley, color);
                                if clipped {
                                    response.on_hover_text(text);
                                }
                            }
                        });
                    });
                });
            });
        });
        ui.add_space(10.0);
    }
    pub(super) fn cleanup(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let phase = if self.operation.is_some() {
            2
        } else if self.plan.is_some() {
            1
        } else {
            0
        };
        kit::steps(
            ui,
            &["Choose files", "Review exact plan", "Inspect outcome"],
            phase,
        );
        ui.add_space(2.0);
        if let Some(operation) = self.operation.clone() {
            self.operation_view(ui, &operation);
            return;
        }
        if let Some(plan) = self.plan.clone() {
            self.plan_view(ui, &plan);
            return;
        }
        kit::banner(
            ui,
            icons::ARCHIVE_BOX,
            "Quarantine moves files into Stratum's private storage on the same filesystem, where they can be restored; on its own it frees no space. Space comes back when you delete a quarantine permanently from its outcome, behind a second typed phrase. Stop active builds before moving generated files.",
            p.amber,
            false,
        );
        if self.cleanup_scope.is_none() {
            self.cleanup_folders(ui);
        } else {
            if let Some(scope) = self.cleanup_scope.clone() {
                ui.horizontal(|ui| {
                    // The two buttons need about 320 points; the path gives way to them.
                    let room = ((ui.available_width() - 360.0) / 6.6).max(16.0) as usize;
                    kit::badge_icon(
                        ui,
                        Some(icons::FUNNEL),
                        &format!("Scope · {}", truncate_middle(&short_path(&scope), room)),
                        p.accent,
                    )
                    .on_hover_text(&scope);
                    if Button::ghost("All cleanup folders")
                        .small()
                        .show(ui)
                        .clicked()
                    {
                        self.cleanup_scope = None;
                        self.offset = 0;
                        self.refresh();
                    }
                    if Button::ghost("Select largest 1,000")
                        .small()
                        .icon(icons::SORT_DESCENDING)
                        .enabled(!self.busy)
                        .show(ui)
                        .on_hover_text(
                            "Replace the selection with the thousand largest candidate files in this folder, the most one plan reviews",
                        )
                        .clicked()
                    {
                        self.task(Activity::Generic, move |e| {
                            Ok(Payload::Selection(
                                e.cleanup_candidates(&FileQuery {
                                    path: Some(scope),
                                    limit: 1000,
                                    ..Default::default()
                                })?
                                .items,
                            ))
                        });
                    }
                });
            }
            kit::caption(
                ui,
                "Nothing is preselected. Candidate rules recognise individual Cargo artifacts and downloaded package-cache files; they do not prove expendability.",
            );
            if self.candidates.is_empty() {
                if self.queries.waiting() {
                    kit::skeleton(ui, 6);
                } else {
                    kit::empty_state(
                        ui,
                        icons::BROOM,
                        if self.has_more {
                            "No candidates on this index page"
                        } else {
                            "No supported candidates on this page"
                        },
                        if self.has_more {
                            "Use Next to continue through the index. Only known candidate rules are included."
                        } else {
                            "A large file is not automatically a cleanup candidate. Try reviewing a development folder from Insights."
                        },
                    );
                }
            }
            let mut groups = BTreeMap::<String, Vec<CleanupCandidate>>::new();
            for candidate in &self.candidates {
                groups
                    .entry(candidate.category.clone())
                    .or_default()
                    .push(candidate.clone());
            }
            for (category, items) in groups {
                let (icon, color) = kit::category_style(&p, &category);
                Card::new().padding(14.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        kit::icon_tile(ui, icon, color, 36.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            kit::badge_icon(
                                ui,
                                Some(icons::ARROW_COUNTER_CLOCKWISE),
                                "Undo supported",
                                p.teal,
                            );
                            kit::badge_icon(ui, Some(icons::WARNING), "Moderate risk", p.amber);
                            kit::badge(
                                ui,
                                &format!("{} files on this page", items.len()),
                                p.text_3,
                            );
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    kit::label(
                                        ui,
                                        humanize(&category),
                                        15.0,
                                        Weight::SemiBold,
                                        p.text,
                                    );
                                    if let Some(first) = items.first() {
                                        kit::label(
                                            ui,
                                            &first.reason,
                                            12.5,
                                            Weight::Regular,
                                            p.text_2,
                                        );
                                    }
                                });
                            });
                        });
                    });
                    ui.add_space(6.0);
                    kit::divider(ui);
                    ui.add_space(2.0);
                    for candidate in &items {
                        let mut checked = self.selected.contains_key(&candidate.path);
                        let response = candidate_row(ui, candidate, &mut checked, !self.busy);
                        if response.changed() {
                            if checked {
                                self.selected.insert(candidate.path.clone(), candidate.size);
                            } else {
                                self.selected.remove(&candidate.path);
                            }
                        }
                    }
                });
            }
            self.pager(ui);
        }
        ui.add_space(8.0);
        kit::disclosure(
            ui,
            "Previous operations: restore or delete",
            "operations",
            |ui| {
                let mut load = None;
                if self.operations.is_empty() {
                    kit::caption(ui, "No quarantine operations have been recorded yet.");
                }
                for operation in &self.operations {
                    let icon = operation_icon(&operation.status);
                    let color = kit::status_color(&p, &operation.status);
                    let held = operation.purgeable_bytes();
                    if Row::new(format!(
                        "{} · {} files{}",
                        humanize(&operation.status),
                        operation.items.len(),
                        if held > 0 {
                            format!(" · {} in quarantine", bytes(held))
                        } else {
                            String::new()
                        }
                    ))
                    .plain_icon(icon, color)
                    .mono_subtitle(&operation.id)
                    .trailing(age(operation.created_at), p.text_3)
                    .height(46.0)
                    .chevron()
                    .show(ui)
                    .clicked()
                    {
                        load = Some(operation.id.clone());
                    }
                }
                ui.horizontal(|ui| {
                    kit::text_field(
                        ui,
                        &mut self.operation_lookup,
                        "Operation ID",
                        Some(icons::MAGNIFYING_GLASS),
                        320.0,
                        true,
                    );
                    if Button::new("Inspect operation")
                        .enabled(!self.busy && !self.operation_lookup.is_empty())
                        .show(ui)
                        .clicked()
                    {
                        load = Some(self.operation_lookup.clone());
                    }
                });
                if let Some(id) = load {
                    self.task(Activity::Generic, move |e| {
                        Ok(Payload::Operation(e.cleanup_operation(&id)?))
                    });
                }
            },
        );
    }
    /// Before any folder is opened: every folder the cleanup rules recognise, largest first,
    /// straight from the index, so the scan from Overview is all this page needs.
    fn cleanup_folders(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        kit::caption(
            ui,
            "Folders the cleanup rules recognise in the index, largest first: Cargo build output beside a Cargo.toml, npm's package cache and Cargo's registry cache. Open one to choose files from it; nothing is preselected.",
        );
        if self.locations.is_empty() {
            if self.queries.waiting() {
                kit::skeleton(ui, 4);
            } else {
                kit::empty_state(
                    ui,
                    icons::BROOM,
                    "No recognised cleanup folders in the index",
                    "Scan a location that holds Rust projects or package caches. Large files on their own are not cleanup candidates.",
                );
            }
            return;
        }
        let total: u64 = self.locations.iter().map(|l| l.logical_bytes).sum();
        let largest = self.locations.first().map_or(1, |l| l.logical_bytes.max(1));
        let mut open = None;
        Card::new().padding(14.0).show(ui, |ui| {
            kit::label(
                ui,
                format!("{} folders · {}", self.locations.len(), bytes(total)),
                15.0,
                Weight::SemiBold,
                p.text,
            );
            kit::caption(ui, "Sizes are the index's totals for each folder; quarantine moves the files you choose, up to a thousand per plan.");
            ui.add_space(6.0);
            kit::divider(ui);
            for location in &self.locations {
                let (icon, color) = kit::category_style(&p, &location.category);
                let response = Row::new(short_path(&location.path))
                    .plain_icon(icon, color)
                    .subtitle(location_kind(&location.category, &location.path))
                    .trailing(bytes(location.logical_bytes), p.text)
                    .height(50.0)
                    .chevron()
                    .show(ui);
                kit::progress(
                    ui,
                    ui.available_width(),
                    3.0,
                    location.logical_bytes as f32 / largest as f32,
                    color,
                );
                ui.add_space(4.0);
                if response.clicked() {
                    open = Some(location.path.clone());
                }
            }
        });
        if let Some(path) = open {
            self.cleanup_scope = Some(path);
            self.offset = 0;
            self.refresh();
        }
    }
    fn plan_view(&mut self, ui: &mut egui::Ui, plan: &CleanupPlan) {
        let p = self.palette;
        let remaining = plan.expires_at.saturating_sub(now()).max(0);
        let expired = remaining == 0;
        Card::new().padding(22.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                kit::icon_tile(ui, icons::LIST_CHECKS, p.accent, 42.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    kit::eyebrow(ui, "Immutable plan · no files have moved");
                    kit::label(
                        ui,
                        format!(
                            "{} across {} files",
                            bytes(plan.total_bytes),
                            plan.items.len()
                        ),
                        24.0,
                        Weight::SemiBold,
                        p.text,
                    );
                    kit::caption(
                        ui,
                        format!(
                            "{} risk · {} · plan {}",
                            plan.risk,
                            if expired {
                                "expired".to_string()
                            } else {
                                format!("expires in {}", duration(remaining))
                            },
                            plan.id
                        ),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    kit::badge_icon(
                        ui,
                        Some(icons::HOURGLASS),
                        &if expired {
                            "Expired".to_string()
                        } else {
                            format!("{} left", duration(remaining))
                        },
                        if expired { p.rose } else { p.amber },
                    );
                });
            });
            ui.add_space(10.0);
            kit::eyebrow(ui, "Exact files in this plan");
            egui::ScrollArea::vertical()
                .id_salt("plan-files")
                .max_height(230.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for item in &plan.items {
                        Row::new(file_name(&item.path))
                            .plain_icon(icons::FILE, p.text_2)
                            .mono_subtitle(short_path(&item.path))
                            .trailing(bytes(item.bytes), p.text)
                            .badge(&item.risk, p.amber)
                            .height(46.0)
                            .enabled(false)
                            .show(ui)
                            .on_hover_text(&item.path);
                    }
                });
            ui.add_space(8.0);
            kit::divider(ui);
            ui.add_space(6.0);
            kit::label(
                ui,
                "Authorization is separate. Type the phrase exactly to approve these files:",
                13.0,
                Weight::Regular,
                p.text_2,
            );
            Card::new()
                .fill(p.sunken)
                .padding(10.0)
                .radius(10.0)
                .shrink()
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(&plan.approval_phrase)
                            .font(fonts::mono_medium(14.0))
                            .color(p.accent),
                    );
                });
            let width = ui.available_width().min(520.0);
            kit::text_field(
                ui,
                &mut self.approval,
                "Type approval phrase",
                Some(icons::LOCK_KEY),
                width,
                true,
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if Button::danger("Authorize quarantine")
                    .icon(icons::ARCHIVE_BOX)
                    .enabled(!self.busy && !expired && self.approval == plan.approval_phrase)
                    .show(ui)
                    .clicked()
                {
                    let approval = self.approval.clone();
                    let plan_id = plan.id.clone();
                    self.task(Activity::Cleanup, move |e| {
                        Ok(Payload::Operation(
                            e.execute_cleanup_plan(&plan_id, &approval)?,
                        ))
                    });
                }
                if Button::ghost("Back to selection")
                    .icon(icons::ARROW_LEFT)
                    .enabled(!self.busy)
                    .show(ui)
                    .clicked()
                {
                    self.plan = None;
                    self.approval.clear();
                }
            });
        });
    }
    fn operation_view(&mut self, ui: &mut egui::Ui, operation: &CleanupOperation) {
        let p = self.palette;
        let color = kit::status_color(&p, &operation.status);
        let held = operation.purgeable().count();
        let held_bytes = operation.purgeable_bytes();
        let purged_bytes = operation.purged_bytes();
        Card::new().padding(22.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                kit::icon_tile(ui, operation_icon(&operation.status), color, 42.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    kit::eyebrow(ui, "Operation outcome");
                    kit::label(
                        ui,
                        format!("Operation {}", humanize(&operation.status)),
                        22.0,
                        Weight::SemiBold,
                        p.text,
                    );
                    ui.horizontal(|ui| {
                        kit::mono(ui, &operation.id, 11.5);
                        if kit::icon_button_ex(
                            ui,
                            icons::COPY,
                            "Copy operation ID",
                            true,
                            24.0,
                            None,
                        )
                        .clicked()
                        {
                            ui.ctx().copy_text(operation.id.clone());
                        }
                    });
                    if let Some(summary) = holdings(operation, held, held_bytes, purged_bytes) {
                        kit::caption(ui, summary);
                    }
                });
            });
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .id_salt("operation-files")
                .max_height(280.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for item in &operation.items {
                        let (icon, color) = item_style(&p, &item.status);
                        Row::new(file_name(&item.source))
                            .plain_icon(icon, color)
                            .mono_subtitle(short_path(&item.source))
                            .trailing(bytes(item.identity.size), p.text_2)
                            .badge(humanize(&item.status), color)
                            .height(46.0)
                            .enabled(false)
                            .show(ui)
                            .on_hover_text(format!("{}\n→ {}", item.source, item.destination));
                        if let Some(error) = &item.error {
                            kit::label(ui, error, 12.0, Weight::Medium, p.rose);
                        }
                    }
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if Button::primary("Restore quarantined files")
                    .icon(icons::ARROW_COUNTER_CLOCKWISE)
                    .enabled(!self.busy && operation.restorable())
                    .show(ui)
                    .clicked()
                {
                    let id = operation.id.clone();
                    self.task(Activity::Cleanup, move |e| {
                        Ok(Payload::Operation(e.undo_cleanup(&id)?))
                    });
                }
                if held > 0
                    && !self.purge_open
                    && Button::danger("Delete permanently…")
                        .icon(icons::TRASH)
                        .enabled(!self.busy)
                        .show(ui)
                        .clicked()
                {
                    self.purge_open = true;
                    self.purge_typed.clear();
                }
                if Button::ghost("Back to candidates")
                    .icon(icons::ARROW_LEFT)
                    .show(ui)
                    .clicked()
                {
                    self.operation = None;
                    self.purge_open = false;
                    self.purge_typed.clear();
                    self.refresh();
                }
            });
            if self.purge_open && held > 0 {
                self.purge_confirmation(ui, operation, held, held_bytes);
            }
        });
    }
    /// The separate, typed authorization for deleting what an operation holds in quarantine.
    fn purge_confirmation(
        &mut self,
        ui: &mut egui::Ui,
        operation: &CleanupOperation,
        held: usize,
        held_bytes: u64,
    ) {
        let p = self.palette;
        let phrase = purge_phrase(&operation.id);
        let wait = self.engine.purge_ready_at(operation).saturating_sub(now());
        ui.add_space(14.0);
        Card::new()
            .tinted(p.rose)
            .padding(16.0)
            .radius(12.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    kit::icon_tile(ui, icons::TRASH, p.rose, 36.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        kit::label(
                            ui,
                            format!(
                                "Delete {held} file{} permanently · {}",
                                if held == 1 { "" } else { "s" },
                                bytes(held_bytes)
                            ),
                            16.0,
                            Weight::SemiBold,
                            p.text,
                        );
                        kit::caption(
                            ui,
                            "This cannot be undone: the files skip the Trash and Stratum cannot restore them. Each file is checked against the content hash recorded when it moved, and anything that changed stays in quarantine.",
                        );
                    });
                });
                ui.add_space(8.0);
                if wait > 0 {
                    kit::banner(
                        ui,
                        icons::HOURGLASS,
                        &format!(
                            "Files stay in quarantine for {} h before they can be deleted. This operation is ready in {}.",
                            self.engine.config.purge_after_hours,
                            duration(wait)
                        ),
                        p.amber,
                        false,
                    );
                } else {
                    kit::label(
                        ui,
                        "Authorization is separate from the quarantine. Type the phrase exactly to delete these files:",
                        13.0,
                        Weight::Regular,
                        p.text_2,
                    );
                    Card::new()
                        .fill(p.sunken)
                        .padding(10.0)
                        .radius(10.0)
                        .shrink()
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&phrase)
                                    .font(fonts::mono_medium(14.0))
                                    .color(p.rose),
                            );
                        });
                    let width = ui.available_width().min(520.0);
                    kit::text_field(
                        ui,
                        &mut self.purge_typed,
                        "Type the purge phrase",
                        Some(icons::LOCK_KEY),
                        width,
                        true,
                    );
                    ui.add_space(4.0);
                }
                ui.horizontal(|ui| {
                    if wait <= 0
                        && Button::danger("Delete permanently")
                            .icon(icons::TRASH)
                            .enabled(!self.busy && self.purge_typed == phrase)
                            .show(ui)
                            .clicked()
                    {
                        let id = operation.id.clone();
                        let approval = self.purge_typed.clone();
                        self.task(Activity::Purge, move |e| {
                            Ok(Payload::Operation(e.purge_quarantine(&id, &approval)?))
                        });
                    }
                    if Button::ghost("Keep in quarantine")
                        .enabled(!self.busy)
                        .show(ui)
                        .clicked()
                    {
                        self.purge_open = false;
                        self.purge_typed.clear();
                    }
                });
            });
    }
}

/// What an operation still holds, and what its purges have deleted.
fn holdings(
    operation: &CleanupOperation,
    held: usize,
    held_bytes: u64,
    purged_bytes: u64,
) -> Option<String> {
    let deleted = match operation.purged_at {
        Some(at) if purged_bytes > 0 => Some(format!(
            "{} deleted permanently {}",
            bytes(purged_bytes),
            age(at)
        )),
        _ => None,
    };
    match (held, deleted) {
        (0, deleted) => deleted,
        (n, deleted) => Some(format!(
            "{n} file{} · {} held in quarantine{}",
            if n == 1 { "" } else { "s" },
            bytes(held_bytes),
            deleted.map(|d| format!(" · {d}")).unwrap_or_default()
        )),
    }
}
fn operation_icon(status: &str) -> &'static str {
    match status {
        "restored" => icons::ARROW_COUNTER_CLOCKWISE,
        "purged" => icons::TRASH,
        _ => icons::ARCHIVE_BOX,
    }
}
fn item_style(p: &Palette, status: &str) -> (&'static str, egui::Color32) {
    match status {
        "restored" => (icons::ARROW_COUNTER_CLOCKWISE, p.teal),
        "quarantined" | "moved" | "completed" => (icons::ARCHIVE_BOX, p.teal),
        "purged" => (icons::TRASH, p.text_2),
        "pending" | "moving" | "purging" => (icons::CIRCLE_DASHED, p.amber),
        "missing" => (icons::QUESTION, p.amber),
        _ => (icons::X_CIRCLE, p.rose),
    }
}

/// A candidate row with a checkbox; clicking anywhere on the row toggles it.
fn candidate_row(
    ui: &mut egui::Ui,
    candidate: &CleanupCandidate,
    checked: &mut bool,
    enabled: bool,
) -> egui::Response {
    let p = kit::palette(ui.ctx());
    let (rect, mut response) = ui.allocate_exact_size(
        vec2(ui.available_width(), 48.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let h = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        enabled && response.hovered(),
        0.12,
    );
    if h > 0.0 || *checked {
        ui.painter().rect_filled(
            rect,
            10.0,
            if *checked {
                p.accent.gamma_multiply(0.08 + 0.04 * h)
            } else {
                p.wash(0.05 * h)
            },
        );
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(10.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 12.0;
    let toggled = kit::checkbox(&mut child, checked, enabled).changed();
    child.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        kit::label(
            ui,
            truncate(&file_name(&candidate.path), 70),
            14.0,
            Weight::Medium,
            p.text,
        );
        kit::mono(ui, truncate_middle(&short_path(&candidate.path), 90), 11.0)
            .on_hover_text(&candidate.path);
    });
    child.with_layout(Layout::right_to_left(Align::Center), |ui| {
        kit::label(ui, bytes(candidate.size), 13.5, Weight::SemiBold, p.accent);
    });
    if enabled && response.clicked() && !toggled {
        *checked = !*checked;
        response.mark_changed();
    } else if toggled {
        response.mark_changed();
    }
    response
}

/// What a recognised folder is and what happens after it is emptied.
fn location_kind(category: &str, path: &str) -> &'static str {
    match category {
        "developer_build_artifact" => "Cargo build output · rebuilt by the next build",
        _ if path.ends_with("/_cacache") => "npm package cache · refilled by the next install",
        _ => "Cargo registry cache · refilled by the next build",
    }
}
