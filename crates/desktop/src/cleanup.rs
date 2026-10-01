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
                    kit::label(
                        ui,
                        format!("{count} selected · {}", bytes(selected_bytes)),
                        14.5,
                        Weight::SemiBold,
                        p.text,
                    );
                    kit::caption(
                        ui,
                        "Selection is for reversible quarantine, not recovered capacity.",
                    );
                });
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
            "Quarantine is reversible storage, not recovered capacity. Files move to Stratum's private storage on the same filesystem; permanent disposal is not supported. Stop active builds before moving generated files.",
            p.amber,
            false,
        );
        if self.cleanup_scope.is_none() {
            self.cleanup_folders(ui);
        } else {
            if let Some(scope) = self.cleanup_scope.clone() {
                ui.horizontal(|ui| {
                    kit::badge_icon(
                        ui,
                        Some(icons::FUNNEL),
                        &format!("Scope · {}", short_path(&scope)),
                        p.accent,
                    );
                    if Button::ghost("All cleanup folders")
                        .small()
                        .show(ui)
                        .clicked()
                    {
                        self.cleanup_scope = None;
                        self.offset = 0;
                        self.refresh();
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
                            kit::badge(ui, &format!("{} files on this page", items.len()), p.text_3);
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    kit::label(ui, humanize(&category), 15.0, Weight::SemiBold, p.text);
                                    if let Some(first) = items.first() {
                                        kit::label(ui, &first.reason, 12.5, Weight::Regular, p.text_2);
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
            "Previous operations & restoration",
            "operations",
            |ui| {
                let mut load = None;
                if self.operations.is_empty() {
                    kit::caption(ui, "No quarantine operations have been recorded yet.");
                }
                for operation in &self.operations {
                    let (icon, color) = if operation.status == "restored" {
                        (icons::ARROW_COUNTER_CLOCKWISE, p.teal)
                    } else {
                        (icons::ARCHIVE_BOX, kit::status_color(&p, &operation.status))
                    };
                    if Row::new(format!(
                        "{} · {} files",
                        humanize(&operation.status),
                        operation.items.len()
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
        Card::new().padding(22.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                kit::icon_tile(
                    ui,
                    if operation.status == "restored" {
                        icons::ARROW_COUNTER_CLOCKWISE
                    } else {
                        icons::ARCHIVE_BOX
                    },
                    color,
                    42.0,
                );
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
                });
            });
            ui.add_space(8.0);
            for item in &operation.items {
                let (icon, color) = match item.status.as_str() {
                    "restored" => (icons::ARROW_COUNTER_CLOCKWISE, p.teal),
                    "moved" | "completed" => (icons::CHECK_CIRCLE, p.teal),
                    "pending" => (icons::CIRCLE_DASHED, p.amber),
                    _ => (icons::X_CIRCLE, p.rose),
                };
                Row::new(file_name(&item.source))
                    .plain_icon(icon, color)
                    .mono_subtitle(short_path(&item.source))
                    .badge(humanize(&item.status), color)
                    .height(46.0)
                    .enabled(false)
                    .show(ui)
                    .on_hover_text(format!("{}\n→ {}", item.source, item.destination));
                if let Some(error) = &item.error {
                    kit::label(ui, error, 12.0, Weight::Medium, p.rose);
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if Button::primary("Restore quarantined files")
                    .icon(icons::ARROW_COUNTER_CLOCKWISE)
                    .enabled(!self.busy && operation.items.iter().any(|i| i.status != "restored"))
                    .show(ui)
                    .clicked()
                {
                    let id = operation.id.clone();
                    self.task(Activity::Cleanup, move |e| {
                        Ok(Payload::Operation(e.undo_cleanup(&id)?))
                    });
                }
                if Button::ghost("Back to candidates")
                    .icon(icons::ARROW_LEFT)
                    .show(ui)
                    .clicked()
                {
                    self.operation = None;
                    self.refresh();
                }
            });
        });
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
