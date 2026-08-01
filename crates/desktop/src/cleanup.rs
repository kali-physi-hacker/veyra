use super::*;
impl App {
    pub(super) fn cleanup_toolbar(&mut self, ui: &mut egui::Ui) {
        if self.plan.is_some() || self.operation.is_some() {
            return;
        }
        let selected_bytes = self.selected.values().copied().sum::<u64>();
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} selected · {}",
                    self.selected.len(),
                    bytes(selected_bytes)
                ))
                .strong(),
            );
            if ui
                .add_enabled(
                    !self.busy && !self.selected.is_empty() && self.selected.len() <= 1000,
                    egui::Button::new("Create review plan").fill(ACCENT.gamma_multiply(0.3)),
                )
                .clicked()
            {
                let paths = self.selected.keys().cloned().collect();
                self.task(move |e| {
                    Ok(Payload::Plan(e.create_cleanup_plan(PlanRequest { paths })?))
                });
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Select this page").small())
                .clicked()
            {
                for candidate in &self.candidates {
                    if self.selected.len() < 1000 {
                        self.selected.insert(candidate.path.clone(), candidate.size);
                    }
                }
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Clear selection").small())
                .clicked()
            {
                self.selected.clear();
            }
        });
        ui.label(
            RichText::new("Selection is for reversible quarantine, not recovered capacity.")
                .small()
                .color(AMBER),
        );
        ui.separator();
    }
    pub(super) fn cleanup(&mut self, ui: &mut egui::Ui) {
        let phase = if self.operation.is_some() {
            3
        } else if self.plan.is_some() {
            2
        } else {
            1
        };
        ui.horizontal_wrapped(|ui| {
            for (number, label) in [
                (1, "Choose files"),
                (2, "Review exact plan"),
                (3, "Inspect outcome"),
            ] {
                pill(
                    ui,
                    &format!("{number}  {label}"),
                    if phase == number { ACCENT } else { MUTED },
                );
            }
        });
        ui.add_space(8.0);
        card().show(ui,|ui|{
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Quarantine is reversible storage, not recovered capacity.").strong().color(AMBER));
            ui.label("Files move to Stratum's private storage on the same filesystem. Permanent disposal is not supported. Stop active builds before moving generated files.");
        });
        if let Some(operation) = self.operation.clone() {
            ui.add_space(12.0);
            ui.heading(format!("Operation: {}", operation.status.replace('_', " ")));
            ui.horizontal(|ui| {
                ui.monospace(&operation.id);
                if ui.small_button("Copy ID").clicked() {
                    ui.ctx().copy_text(operation.id.clone());
                }
            });
            for item in &operation.items {
                ui.label(format!(
                    "{} · {}",
                    item.status.replace('_', " "),
                    short_path(&item.source)
                ));
                if let Some(error) = &item.error {
                    ui.colored_label(AMBER, error);
                }
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.busy && operation.items.iter().any(|i| i.status != "restored"),
                        egui::Button::new("Restore quarantined files"),
                    )
                    .clicked()
                {
                    self.task(move |e| Ok(Payload::Operation(e.undo_cleanup(&operation.id)?)));
                }
                if ui.button("Back to candidates").clicked() {
                    self.operation = None;
                    self.refresh();
                }
            });
            return;
        }
        if let Some(plan) = self.plan.clone() {
            ui.add_space(12.0);
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                eyebrow(ui, "Immutable plan · no files have moved");
                ui.label(
                    RichText::new(format!(
                        "{} across {} files",
                        bytes(plan.total_bytes),
                        plan.items.len()
                    ))
                    .size(26.0)
                    .strong(),
                );
                ui.label(format!(
                    "{} risk · expires in {} minutes",
                    plan.risk,
                    plan.expires_at.saturating_sub(now()).max(0) / 60
                ));
                egui::CollapsingHeader::new("Exact files in this plan")
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("plan-files")
                            .max_height(220.0)
                            .show(ui, |ui| {
                                for item in &plan.items {
                                    ui.label(format!(
                                        "{}  {}",
                                        bytes(item.bytes),
                                        short_path(&item.path)
                                    ))
                                    .on_hover_text(&item.path);
                                }
                            });
                    });
                ui.label(
                    "Authorization is separate. Type the phrase exactly to approve these files:",
                );
                ui.label(
                    RichText::new(&plan.approval_phrase)
                        .monospace()
                        .color(ACCENT),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.approval)
                        .desired_width(ui.available_width())
                        .hint_text("Type approval phrase"),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.busy
                                && now() < plan.expires_at
                                && self.approval == plan.approval_phrase,
                            egui::Button::new("Authorize quarantine")
                                .fill(Color32::from_rgb(102, 68, 39)),
                        )
                        .clicked()
                    {
                        let approval = self.approval.clone();
                        self.task(move |e| {
                            Ok(Payload::Operation(
                                e.execute_cleanup_plan(&plan.id, &approval)?,
                            ))
                        });
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Back to selection"))
                        .clicked()
                    {
                        self.plan = None;
                        self.approval.clear();
                    }
                });
            });
            return;
        }
        ui.add_space(12.0);
        if let Some(scope) = self.cleanup_scope.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Scope: {}", short_path(&scope)));
                if ui.small_button("All indexed locations").clicked() {
                    self.cleanup_scope = None;
                    self.offset = 0;
                    self.refresh();
                }
            });
        }
        ui.label(RichText::new("Nothing is preselected. Candidate rules recognize individual Cargo artifacts and downloaded package-cache files; they do not prove expendability.").small().color(MUTED));
        if self.candidates.is_empty() && !self.queries.loading() {
            empty(
                ui,
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
        let mut groups = BTreeMap::<&str, Vec<&CleanupCandidate>>::new();
        for candidate in &self.candidates {
            groups
                .entry(&candidate.category)
                .or_default()
                .push(candidate);
        }
        for (category, items) in groups {
            ui.add_space(10.0);
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    eyebrow(ui, &category.replace('_', " "));
                    ui.label(format!("{} files on this page", items.len()));
                    pill(ui, "Moderate risk", AMBER);
                    pill(ui, "Undo supported", TEAL);
                });
                if let Some(first) = items.first() {
                    ui.label(RichText::new(&first.reason).small().color(MUTED));
                }
                ui.separator();
                for candidate in items {
                    ui.horizontal(|ui| {
                        let mut checked = self.selected.contains_key(&candidate.path);
                        let name = std::path::Path::new(&candidate.path)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy();
                        if ui
                            .add_enabled(!self.busy, egui::Checkbox::new(&mut checked, ""))
                            .on_hover_text("Include this exact file in review")
                            .changed()
                        {
                            if checked {
                                self.selected.insert(candidate.path.clone(), candidate.size);
                            } else {
                                self.selected.remove(&candidate.path);
                            }
                        }
                        ui.vertical(|ui| {
                            ui.label(RichText::new(truncate(&name, 65)).strong());
                            ui.label(
                                RichText::new(truncate(&short_path(&candidate.path), 90))
                                    .size(11.0)
                                    .color(MUTED),
                            )
                            .on_hover_text(&candidate.path);
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(bytes(candidate.size)).color(ACCENT));
                        });
                    });
                    ui.separator();
                }
            });
        }
        self.pager(ui);
        ui.add_space(16.0);
        egui::CollapsingHeader::new("Previous operations & restoration").show(ui, |ui| {
            let mut load = None;
            for operation in &self.operations {
                if ui
                    .button(format!(
                        "{} · {} · {} files",
                        operation.status,
                        operation.id,
                        operation.items.len()
                    ))
                    .clicked()
                {
                    load = Some(operation.id.clone());
                }
            }
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.operation_lookup)
                        .hint_text("Operation ID"),
                );
                if ui
                    .add_enabled(
                        !self.busy && !self.operation_lookup.is_empty(),
                        egui::Button::new("Inspect operation"),
                    )
                    .clicked()
                {
                    load = Some(self.operation_lookup.clone());
                }
            });
            if let Some(id) = load {
                self.task(move |e| Ok(Payload::Operation(e.cleanup_operation(&id)?)));
            }
        });
    }
}
