use super::*;
use eframe::egui::{Align, Layout};
use kit::{Card, Row};

impl App {
    pub(super) fn audit(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        if self.audit.is_empty() {
            if self.queries.waiting() {
                kit::skeleton(ui, 6);
            } else {
                kit::empty_state(
                    ui,
                    icons::CLIPBOARD_TEXT,
                    "Your actions leave a record",
                    "Scans, plans, moves and restores will appear here.",
                );
            }
            return;
        }
        let records = self.audit.clone();
        let mut toggle = None;
        Card::new().padding(8.0).show(ui, |ui| {
            for record in &records {
                let (icon, color) = kit::audit_style(&p, &record.action);
                let expanded = self.expanded_audit == Some(record.id);
                let response = Row::new(humanize(&record.action))
                    .plain_icon(icon, color)
                    .mono_subtitle(truncate_middle(&record.resource_id, 72))
                    .trailing(age(record.timestamp), p.text_3)
                    .selected(expanded)
                    .chevron()
                    .height(48.0)
                    .show(ui)
                    .on_hover_text(datetime(record.timestamp));
                if response.clicked() {
                    toggle = Some(record.id);
                }
                if expanded {
                    Card::new()
                        .fill(p.sunken)
                        .padding(12.0)
                        .radius(10.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                kit::eyebrow(ui, "Recorded detail");
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if kit::icon_button_ex(
                                        ui,
                                        icons::COPY,
                                        "Copy detail",
                                        true,
                                        24.0,
                                        None,
                                    )
                                    .clicked()
                                    {
                                        ui.ctx().copy_text(record.detail.clone());
                                    }
                                });
                            });
                            let mut job = eframe::egui::text::LayoutJob::simple(
                                record.detail.clone(),
                                fonts::mono(12.0),
                                p.text_2,
                                ui.available_width(),
                            );
                            job.wrap.break_anywhere = true;
                            ui.label(job);
                            kit::caption(ui, format!("Resource {}", record.resource_id));
                        });
                    ui.add_space(4.0);
                }
            }
        });
        if let Some(id) = toggle {
            self.expanded_audit = if self.expanded_audit == Some(id) {
                None
            } else {
                Some(id)
            };
        }
        self.pager(ui);
    }
}
