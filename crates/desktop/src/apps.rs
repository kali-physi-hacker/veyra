use super::*;
use eframe::egui::{Align, Layout};
use kit::{Button, Card, Row};

impl App {
    pub(super) fn apps(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        if self.apps.is_empty() {
            if self.queries.loading() {
                kit::skeleton(ui, 6);
            } else {
                kit::empty_state(
                    ui,
                    icons::APP_WINDOW,
                    "No indexed application bundles",
                    "Scan /Applications and relevant Library locations. Footprints only include paths actually observed.",
                );
            }
        }
        let mut requested = None;
        let mut reveal = None;
        let apps: Vec<Application> = self.apps.iter().take(100).cloned().collect();
        let total: u64 = apps.iter().map(|a| a.footprint_bytes).sum();
        if !apps.is_empty() {
            ui.horizontal(|ui| {
                kit::badge_icon(
                    ui,
                    Some(icons::APP_WINDOW),
                    &format!("{} bundles", apps.len()),
                    p.rose,
                );
                kit::badge_icon(
                    ui,
                    Some(icons::DATABASE),
                    &format!("{} estimated footprint", bytes(total)),
                    p.accent,
                );
                kit::caption(ui, "Largest footprints first");
            });
        }
        for app in &apps {
            let color = p.series(hash(&app.name));
            Card::new().hover().padding(16.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    kit::icon_tile(ui, icons::APP_WINDOW, color, 44.0);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                kit::label(
                                    ui,
                                    bytes(app.footprint_bytes),
                                    20.0,
                                    Weight::SemiBold,
                                    p.text,
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                kit::caption(ui, "estimated footprint");
                            });
                        });
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                kit::label(ui, &app.name, 17.0, Weight::SemiBold, p.text);
                                if let Some(id) = &app.bundle_id {
                                    kit::mono(ui, id, 11.5);
                                }
                                kit::caption(ui, &app.coverage);
                            });
                        });
                    });
                });
                kit::disclosure(
                    ui,
                    &format!("{} observed storage locations", app.associations.len()),
                    &app.id,
                    |ui| {
                        for association in &app.associations {
                            let (icon, color) = association_style(&p, &association.kind);
                            Row::new(humanize(&association.kind))
                                .plain_icon(icon, color)
                                .mono_subtitle(short_path(&association.path))
                                .trailing(bytes(association.bytes), p.text)
                                .badge(&association.confidence, p.teal)
                                .height(46.0)
                                .enabled(false)
                                .show(ui)
                                .on_hover_text(&association.path);
                            for evidence in &association.evidence {
                                ui.horizontal(|ui| {
                                    ui.add_space(30.0);
                                    kit::caption(ui, &evidence.detail);
                                });
                            }
                        }
                    },
                );
                ui.horizontal(|ui| {
                    if Button::new("Reveal application")
                        .icon(icons::ARROW_SQUARE_OUT)
                        .small()
                        .show(ui)
                        .clicked()
                    {
                        reveal = Some(app.path.clone());
                    }
                    if Button::soft("Create uninstall review", p.amber)
                        .icon(icons::LIST_CHECKS)
                        .small()
                        .enabled(!self.busy)
                        .show(ui)
                        .clicked()
                    {
                        requested = Some(app.id.clone());
                    }
                });
            });
        }
        if self.apps.len() > 100 {
            kit::caption(
                ui,
                "Showing the largest 100 indexed applications; the API exposes the full bounded inventory.",
            );
        }
        if let Some(path) = reveal {
            self.reveal(&path);
        }
        if let Some(id) = requested {
            self.task(Activity::Generic, move |e| {
                Ok(Payload::Uninstall(e.uninstall_plan(&id)?))
            });
        }
        if let Some(review) = self.uninstall.clone() {
            Card::new().tinted(p.amber).show(ui, |ui| {
                ui.horizontal(|ui| {
                    kit::icon_tile(ui, icons::LIST_CHECKS, p.amber, 36.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        kit::eyebrow(ui, "Uninstall review · not executable");
                        kit::label(
                            ui,
                            review["application"]["name"]
                                .as_str()
                                .unwrap_or("Application"),
                            16.0,
                            Weight::SemiBold,
                            p.text,
                        );
                        kit::label(
                            ui,
                            review["reason"].as_str().unwrap_or(
                                "Review the observed associations. No files were changed.",
                            ),
                            13.0,
                            Weight::Regular,
                            p.text_2,
                        );
                        if let Some(id) = review["id"].as_str() {
                            ui.horizontal(|ui| {
                                kit::mono(ui, id, 11.5);
                                if kit::icon_button_ex(
                                    ui,
                                    icons::COPY,
                                    "Copy report ID",
                                    true,
                                    24.0,
                                    None,
                                )
                                .clicked()
                                {
                                    ui.ctx().copy_text(id.into());
                                }
                            });
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        if kit::icon_button(ui, icons::X, "Dismiss review").clicked() {
                            self.uninstall = None;
                        }
                    });
                });
            });
        }
        kit::caption(
            ui,
            "Bundle removal is not supported. Association evidence is not proof that shared data can be discarded.",
        );
    }
}

fn hash(name: &str) -> usize {
    name.bytes().fold(7usize, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(b as usize)
    })
}

fn association_style(p: &Palette, kind: &str) -> (&'static str, Color32) {
    let kind = kind.to_ascii_lowercase();
    if kind.contains("cache") {
        (icons::STACK, p.amber)
    } else if kind.contains("log") {
        (icons::RECEIPT, p.text_3)
    } else if kind.contains("preference") || kind.contains("setting") {
        (icons::GEAR_SIX, p.cyan)
    } else if kind.contains("container") {
        (icons::CUBE, p.cyan)
    } else if kind.contains("bundle") || kind.contains("app") {
        (icons::APP_WINDOW, p.rose)
    } else if kind.contains("support") || kind.contains("data") {
        (icons::DATABASE, p.accent)
    } else {
        (icons::FOLDER_SIMPLE, p.teal)
    }
}
