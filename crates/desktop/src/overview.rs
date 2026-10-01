use super::*;
use eframe::egui::{Align, Layout, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};
use kit::{Button, Card};

impl App {
    pub(super) fn overview(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let Some(value) = &self.overview else {
            if self.queries.waiting() {
                kit::skeleton(ui, 8);
            } else {
                kit::empty_state(
                    ui,
                    icons::HOUSE,
                    "Reading your local workspace",
                    "Loading saved observations. No scan or cleanup starts automatically.",
                );
            }
            return;
        };
        let resources = value.resources.clone();
        let categories = value.categories.clone();
        let coverage = value.coverage.clone();
        let findings: Vec<_> = value.insights.iter().take(3).cloned().collect();
        if coverage.is_empty() {
            self.onboarding(ui);
            return;
        }
        let t = self.reveal;
        let total = categories.iter().map(|c| c.logical_bytes).sum::<u64>();
        let files = categories.iter().map(|c| c.files).sum::<u64>();
        let warnings = coverage.iter().map(|s| s.warnings).sum::<u64>();
        let volume = resources
            .volumes
            .iter()
            .find(|v| v.mount == "/")
            .or_else(|| resources.volumes.first())
            .cloned();

        Card::new().padding(24.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(v) = &volume {
                    let used = 1.0 - v.available_bytes as f32 / v.total_bytes.max(1) as f32;
                    kit::ring(
                        ui,
                        176.0,
                        13.0,
                        used * t,
                        p.accent,
                        p.accent_2,
                        &bytes(v.available_bytes),
                        "available",
                    );
                    ui.add_space(20.0);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    match &volume {
                        Some(v) => kit::eyebrow(ui, &format!("{} · {}", v.name, v.filesystem)),
                        None => kit::eyebrow(ui, "Your machine, in perspective"),
                    };
                    kit::label(ui, "Know where your space goes.", 27.0, Weight::Bold, p.text);
                    if let Some(v) = &volume {
                        kit::label(
                            ui,
                            format!(
                                "{} used of {}",
                                bytes(v.total_bytes.saturating_sub(v.available_bytes)),
                                bytes(v.total_bytes)
                            ),
                            14.0,
                            Weight::Medium,
                            p.text_2,
                        );
                    }
                    kit::label(
                        ui,
                        "Volume capacity is an OS measurement. Indexed logical bytes are a different measurement, not a complete physical-disk accounting.",
                        12.5,
                        Weight::Regular,
                        p.text_3,
                    );
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if Button::new("Explore storage")
                            .icon(icons::SQUARES_FOUR)
                            .show(ui)
                            .clicked()
                        {
                            self.choose_page(Page::Map);
                        }
                        if Button::new("Review findings")
                            .icon(icons::LIGHTBULB)
                            .show(ui)
                            .clicked()
                        {
                            self.choose_page(Page::Insights);
                        }
                        kit::badge_icon(ui, Some(icons::SHIELD_CHECK), "Private by design", p.teal);
                    });
                });
            });
        });

        ui.columns(3, |cols| {
            kit::metric(
                &mut cols[0],
                "Indexed storage",
                &bytes((total as f64 * t as f64) as u64),
                &format!("{} files · {} roots", util::count(files), coverage.len()),
                icons::DATABASE,
                p.accent,
            );
            kit::metric(
                &mut cols[1],
                "Coverage",
                &if warnings == 0 {
                    "No warnings".to_string()
                } else {
                    format!("{warnings} warnings")
                },
                "Only selected roots are indexed",
                if warnings == 0 {
                    icons::SEAL_CHECK
                } else {
                    icons::WARNING
                },
                if warnings == 0 { p.teal } else { p.amber },
            );
            kit::metric(
                &mut cols[2],
                "Resource snapshot",
                &format!("{:.0}% CPU", resources.cpu_percent),
                &format!(
                    "{} memory used · {}",
                    bytes(resources.used_memory),
                    age(resources.timestamp)
                ),
                icons::CPU,
                p.green,
            );
        });

        ui.add_space(6.0);
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            kit::section(
                ui,
                "What occupies the index",
                "Logical sizes · categories can be heuristic",
                |_| {},
            );
            Card::new().show(ui, |ui| {
                let top: Vec<_> = categories.iter().take(7).collect();
                let shown: u64 = top.iter().map(|c| c.logical_bytes).sum();
                let mut segments: Vec<(f32, Color32)> = top
                    .iter()
                    .map(|c| {
                        (
                            percent(c.logical_bytes, total) * t,
                            kit::category_style(&p, &c.category).1,
                        )
                    })
                    .collect();
                if total > shown {
                    segments.push((percent(total - shown, total) * t, p.text_3));
                }
                kit::stacked_bar(ui, 12.0, &segments);
                ui.add_space(8.0);
                for c in &top {
                    let (icon, color) = kit::category_style(&p, &c.category);
                    ui.horizontal(|ui| {
                        kit::icon_tile(ui, icon, color, 28.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            ui.horizontal(|ui| {
                                kit::label(
                                    ui,
                                    humanize(&c.category),
                                    13.5,
                                    Weight::Medium,
                                    p.text,
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    kit::label(
                                        ui,
                                        bytes(c.logical_bytes),
                                        13.0,
                                        Weight::SemiBold,
                                        p.text,
                                    );
                                    kit::label(
                                        ui,
                                        format!(
                                            "{:.0}% · {} files",
                                            percent(c.logical_bytes, total) * 100.0,
                                            util::count(c.files)
                                        ),
                                        12.0,
                                        Weight::Regular,
                                        p.text_3,
                                    );
                                });
                            });
                            let width = ui.available_width();
                            kit::progress(ui, width, 5.0, percent(c.logical_bytes, total) * t, color);
                        });
                    });
                    ui.add_space(2.0);
                }
                if categories.is_empty() {
                    kit::muted(ui, "No category totals recorded yet.");
                }
            });
            let ui = &mut cols[1];
            kit::section(
                ui,
                "Worth your attention",
                "Observed facts, with a path to investigate",
                |_| {},
            );
            if findings.is_empty() {
                kit::empty_state(
                    ui,
                    icons::SEAL_CHECK,
                    "A baseline, not a verdict",
                    "No rules produced findings in this scope. Rescan later to observe changes; absence of findings is not a system health assessment.",
                );
            }
            for insight in &findings {
                self.finding_card(ui, insight, true);
            }
        });

        ui.add_space(6.0);
        kit::section(
            ui,
            "Your indexed locations",
            "Each root is indexed independently. Rescan to refresh a location.",
            |_| {},
        );
        for scan in &coverage {
            self.location_card(ui, scan);
        }
    }

    fn location_card(&mut self, ui: &mut egui::Ui, scan: &ScanRecord) {
        let p = self.palette;
        let stale = scan.freshness == "stale";
        Card::new().hover().padding(14.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                kit::icon_tile(ui, icons::FOLDER, p.teal, 38.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.horizontal(|ui| {
                        kit::label(ui, short_path(&scan.root), 14.5, Weight::SemiBold, p.text)
                            .on_hover_text(&scan.root);
                        kit::badge(
                            ui,
                            &humanize(&scan.freshness),
                            if stale { p.amber } else { p.teal },
                        );
                    });
                    kit::label(
                        ui,
                        format!(
                            "{} · {} entries · {} warnings · {} excluded",
                            bytes(scan.logical_bytes),
                            util::count(scan.entries),
                            scan.warnings,
                            scan.excluded
                        ),
                        12.5,
                        Weight::Regular,
                        p.text_2,
                    );
                    kit::caption(
                        ui,
                        format!(
                            "{} · last full scan {}",
                            humanize(&scan.status),
                            scan.completed_at.map_or_else(|| "unknown".into(), age)
                        ),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if Button::new("Rescan")
                        .icon(icons::ARROWS_CLOCKWISE)
                        .small()
                        .enabled(!self.busy)
                        .show(ui)
                        .clicked()
                    {
                        self.root = scan.root.clone();
                        self.start_scan();
                    }
                    if Button::soft("Explore", p.teal)
                        .icon(icons::SQUARES_FOUR)
                        .small()
                        .show(ui)
                        .clicked()
                    {
                        self.open_location(Page::Map, scan.root.clone());
                    }
                });
            });
        });
    }

    fn onboarding(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let t = self.reveal;
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
        Card::new().padding(36.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(560.0);
                    ui.spacing_mut().item_spacing.y = 12.0;
                    kit::badge_icon(
                        ui,
                        Some(icons::SPARKLE),
                        "A clearer picture starts with one folder",
                        p.accent,
                    );
                    kit::label(
                        ui,
                        "Meet your storage.\nUnderstand what matters.",
                        36.0,
                        Weight::Bold,
                        p.text,
                    );
                    kit::label(
                        ui,
                        "Choose a folder to build your private, reusable index. Explore sizes, recognise development artifacts, and start a history of what changes.",
                        14.5,
                        Weight::Regular,
                        p.text_2,
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if Button::primary("Choose a folder…")
                            .icon(icons::FOLDER_OPEN)
                            .show(ui)
                            .clicked()
                        {
                            self.show_scan_dialog = true;
                        }
                        if Button::new("Start with Downloads")
                            .icon(icons::DOWNLOAD_SIMPLE)
                            .show(ui)
                            .clicked()
                        {
                            self.root = home.join("Downloads").display().to_string();
                            self.show_scan_dialog = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        kit::icon(ui, icons::SHIELD_CHECK, 15.0, p.teal);
                        kit::label(
                            ui,
                            "Read-only discovery. No account. No cloud upload. Nothing is selected for cleanup.",
                            12.5,
                            Weight::Medium,
                            p.teal,
                        );
                    });
                });
                if ui.available_width() > 280.0 {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        onboarding_art(ui, &p, t);
                    });
                }
            });
        });
        ui.add_space(4.0);
        ui.columns(3, |cols| {
            let steps = [
                (
                    "01",
                    icons::SCAN,
                    p.accent,
                    "Observe",
                    "Scan one folder or a volume. Permission gaps stay visible.",
                ),
                (
                    "02",
                    icons::SQUARES_FOUR,
                    p.teal,
                    "Understand",
                    "Explore the map and the evidence behind each finding.",
                ),
                (
                    "03",
                    icons::LIST_CHECKS,
                    p.amber,
                    "Decide",
                    "Review exact files before any supported, reversible action.",
                ),
            ];
            for (ui, (number, icon, color, title, detail)) in cols.iter_mut().zip(steps) {
                Card::new().hover().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        kit::icon_tile(ui, icon, color, 36.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            kit::eyebrow(ui, number);
                            kit::label(ui, title, 15.0, Weight::SemiBold, p.text);
                            kit::label(ui, detail, 12.5, Weight::Regular, p.text_2);
                        });
                    });
                });
            }
        });
    }

    pub(super) fn finding_card(&mut self, ui: &mut egui::Ui, insight: &Insight, compact: bool) {
        let p = self.palette;
        let (icon, color) = kit::insight_style(&p, &insight.kind, &insight.severity);
        let mut investigate = None;
        let mut review = None;
        Card::new().hover().padding(16.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                kit::icon_tile(ui, icon, color, 34.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.horizontal(|ui| {
                        kit::eyebrow(
                            ui,
                            &format!(
                                "{} · {:.0}% confidence · {} risk",
                                humanize(&insight.kind),
                                insight.confidence * 100.0,
                                insight.risk
                            ),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            kit::badge(ui, &bytes(insight.estimated_impact), color);
                        });
                    });
                    kit::label(ui, &insight.title, 15.0, Weight::SemiBold, p.text);
                    if !compact {
                        kit::label(ui, &insight.description, 13.0, Weight::Regular, p.text_2);
                    }
                    if let Some(path) = insight.related_resources.first() {
                        kit::mono_truncated(ui, &short_path(path), 11.5).on_hover_text(path);
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if Button::soft("Investigate", p.accent)
                    .icon(icons::BINOCULARS)
                    .small()
                    .show(ui)
                    .clicked()
                    && let Some(path) = insight.related_resources.first()
                {
                    investigate = Some(
                        if insight.possible_actions.iter().any(|a| a == "inspect_file") {
                            parent(path)
                        } else {
                            path.clone()
                        },
                    );
                }
                if insight
                    .possible_actions
                    .iter()
                    .any(|a| a == "create_cleanup_plan")
                    && Button::soft("Review candidates", p.amber)
                        .icon(icons::BROOM)
                        .small()
                        .show(ui)
                        .clicked()
                {
                    review = insight.related_resources.first().cloned();
                }
            });
            kit::disclosure(ui, "Why this finding?", &insight.id, |ui| {
                if compact {
                    kit::label(ui, &insight.description, 13.0, Weight::Regular, p.text_2);
                }
                for e in &insight.evidence {
                    kit::label(ui, humanize(&e.mechanism), 12.0, Weight::SemiBold, color);
                    kit::label(ui, &e.detail, 13.0, Weight::Regular, p.text_2);
                }
                kit::label(
                    ui,
                    "Impact values can overlap. Do not add them together as reclaimable space.",
                    12.0,
                    Weight::Medium,
                    p.amber,
                );
            });
        });
        if let Some(path) = investigate {
            self.open_location(Page::Explorer, path);
        }
        if let Some(path) = review {
            self.cleanup_scope = Some(path);
            self.selected.clear();
            self.plan = None;
            self.operation = None;
            self.choose_page(Page::Cleanup);
        }
    }
}

/// A decorative miniature storage map that assembles itself on first paint.
fn onboarding_art(ui: &mut egui::Ui, p: &Palette, t: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(250.0, 190.0), Sense::hover());
    let painter = ui.painter();
    kit::glow(
        ui,
        rect.shrink(10.0),
        24.0,
        p.accent.gamma_multiply(0.18 * t),
        40.0,
    );
    let tiles: [(f32, f32, f32, f32, usize); 6] = [
        (0.0, 0.0, 0.58, 0.62, 0),
        (0.60, 0.0, 0.40, 0.36, 1),
        (0.60, 0.38, 0.40, 0.24, 2),
        (0.0, 0.64, 0.30, 0.36, 3),
        (0.32, 0.64, 0.26, 0.36, 4),
        (0.60, 0.64, 0.40, 0.36, 5),
    ];
    for (index, (x, y, w, h, series)) in tiles.iter().enumerate() {
        let delay = index as f32 * 0.08;
        let local = ((t - delay) / (1.0 - delay).max(0.01)).clamp(0.0, 1.0);
        let local = eframe::egui::emath::easing::cubic_out(local);
        let tile = Rect::from_min_size(
            pos2(
                rect.left() + x * rect.width(),
                rect.top() + y * rect.height(),
            ),
            vec2(w * rect.width(), h * rect.height()),
        )
        .shrink(3.0);
        let tile = Rect::from_center_size(tile.center(), tile.size() * (0.6 + 0.4 * local));
        let color = p.series(*series);
        painter.rect(
            tile,
            8.0,
            color.gamma_multiply(0.35 * local),
            Stroke::new(1.0, color.gamma_multiply(0.7 * local)),
            StrokeKind::Inside,
        );
        if index == 0 {
            painter.rect_filled(
                Rect::from_min_size(tile.min + Vec2::splat(12.0), vec2(48.0, 6.0)),
                3.0,
                Color32::from_white_alpha((150.0 * local) as u8),
            );
            painter.rect_filled(
                Rect::from_min_size(tile.min + vec2(12.0, 24.0), vec2(30.0, 6.0)),
                3.0,
                Color32::from_white_alpha((90.0 * local) as u8),
            );
        }
    }
}
