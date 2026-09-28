//! The application chrome: sidebar navigation, page header, the scan modal and the floating
//! activity card. Pages render inside `content`.
use super::*;
use eframe::egui::{
    Align, Align2, Frame, Id, Layout, Margin, Order, Rect, Sense, Stroke, StrokeKind, Vec2,
    ViewportCommand, epaint, pos2, vec2,
};
use kit::{Button, Card};

const SIDEBAR_WIDTH: f32 = 236.0;

impl App {
    pub(super) fn sidebar(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let top = if cfg!(target_os = "macos") { 40 } else { 16 };
        egui::SidePanel::left("navigation")
            .exact_width(SIDEBAR_WIDTH)
            .resizable(false)
            .frame(Frame::new().fill(p.sidebar).inner_margin(Margin {
                left: 14,
                right: 14,
                top,
                bottom: 14,
            }))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                self.brand_row(ui);
                ui.add_space(16.0);
                for (section, pages) in Page::sections() {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        kit::eyebrow(ui, section);
                    });
                    ui.add_space(2.0);
                    for &page in pages {
                        self.nav_item(ui, page);
                    }
                    ui.add_space(8.0);
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    self.sidebar_footer(ui);
                });
            });
        let painter = ctx.layer_painter(egui::LayerId::background());
        let screen = ctx.content_rect();
        painter.vline(
            screen.left() + SIDEBAR_WIDTH,
            screen.y_range(),
            Stroke::new(1.0, p.border),
        );
    }
    fn brand_row(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::drag());
        if response.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let painter = ui.painter();
        let mark =
            Rect::from_min_size(pos2(rect.left() + 8.0, rect.top() + 4.0), Vec2::splat(36.0));
        brand::paint_mark(painter, mark, &p);
        painter.text(
            pos2(mark.right() + 12.0, rect.top() + 12.0),
            Align2::LEFT_CENTER,
            "Stratum",
            fonts::font(18.0, Weight::SemiBold),
            p.text,
        );
        painter.text(
            pos2(mark.right() + 12.0, rect.top() + 31.0),
            Align2::LEFT_CENTER,
            "Local machine intelligence",
            fonts::font(11.0, Weight::Medium),
            p.text_3,
        );
    }
    fn nav_item(&mut self, ui: &mut egui::Ui, page: Page) {
        let p = self.palette;
        let active = self.page == page;
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let a = ui
            .ctx()
            .animate_bool_with_time(response.id.with("active"), active, 0.18);
        let h =
            ui.ctx()
                .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
        let painter = ui.painter();
        if a > 0.0 {
            painter.rect(
                rect,
                11.0,
                p.accent.gamma_multiply(0.16 * a),
                Stroke::new(1.0, p.accent.gamma_multiply(0.38 * a)),
                StrokeKind::Inside,
            );
        }
        if h > 0.0 && a < 1.0 {
            painter.rect_filled(rect, 11.0, p.wash(0.05 * h * (1.0 - a)));
        }
        let tile =
            Rect::from_center_size(pos2(rect.left() + 23.0, rect.center().y), Vec2::splat(26.0));
        kit::paint_icon_tile(painter, tile, page.icon(), page.color(&p));
        let color = p.text_2.lerp_to_gamma(p.text, a.max(h * 0.6));
        painter.text(
            pos2(rect.left() + 46.0, rect.center().y),
            Align2::LEFT_CENTER,
            page.title(),
            fonts::font(
                13.5,
                if active {
                    Weight::SemiBold
                } else {
                    Weight::Medium
                },
            ),
            color,
        );
        if response.clicked() {
            self.choose_page(page);
        }
    }
    fn sidebar_footer(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let time = ui.input(|i| i.time) as f32;
            let pulse = if self.busy {
                ui.ctx().request_repaint();
                0.55 + 0.45 * (time * 4.0).sin()
            } else {
                1.0
            };
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            let color = if self.busy { p.accent } else { p.teal };
            ui.painter()
                .circle_filled(dot.center(), 3.5, color.gamma_multiply(pulse));
            let status = kit::galley_truncated(
                ui,
                &self.status,
                fonts::font(11.5, Weight::Regular),
                p.text_3,
                ui.available_width() - 44.0,
            );
            let (rect, response) =
                ui.allocate_exact_size(vec2(status.size().x, 20.0), Sense::hover());
            ui.painter().galley(
                pos2(rect.left(), rect.center().y - status.size().y / 2.0),
                status,
                p.text_3,
            );
            response.on_hover_text(&self.status);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let dark = p.dark;
                let response = kit::icon_button_ex(
                    ui,
                    if dark { icons::SUN } else { icons::MOON },
                    if dark {
                        "Switch to light appearance"
                    } else {
                        "Switch to dark appearance"
                    },
                    true,
                    30.0,
                    None,
                );
                if response.clicked() {
                    self.set_dark(!dark);
                }
            });
        });
        Card::new().padding(12.0).radius(12.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                kit::icon_tile(ui, icons::SHIELD_CHECK, p.teal, 30.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    kit::label(ui, "On this device", 12.5, Weight::SemiBold, p.text);
                    kit::label(
                        ui,
                        "No telemetry · no account",
                        11.0,
                        Weight::Regular,
                        p.text_3,
                    );
                });
            });
        });
    }
    pub(super) fn content(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg).inner_margin(Margin {
                left: 30,
                right: 30,
                top: if cfg!(target_os = "macos") { 34 } else { 22 },
                bottom: 0,
            }))
            .show(ctx, |ui| {
                self.header(ui);
                ui.add_space(14.0);
                if let Some(error) = self.error.clone()
                    && kit::banner(ui, icons::WARNING_CIRCLE, &error, p.rose, true)
                {
                    self.error = None;
                }
                if self.error.is_some() {
                    ui.add_space(10.0);
                }
                if self.page == Page::Cleanup {
                    self.cleanup_toolbar(ui);
                }
                let reveal = self.reveal;
                egui::ScrollArea::vertical()
                    .id_salt((
                        self.page.title(),
                        self.plan.as_ref().map(|p| p.id.as_str()),
                        self.operation.as_ref().map(|o| o.id.as_str()),
                    ))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        kit::reveal(ui, reveal);
                        ui.spacing_mut().item_spacing.y = 12.0;
                        ui.set_width(ui.available_width());
                        match self.page {
                            Page::Overview => self.overview(ui),
                            Page::Map => self.map(ui),
                            Page::Explorer => self.explorer(ui),
                            Page::Cleanup => self.cleanup(ui),
                            Page::Apps => self.apps(ui),
                            Page::Duplicates => self.duplicates(ui),
                            Page::System => self.system(ui),
                            Page::Insights => self.insights(ui),
                            Page::History => self.history(ui),
                            Page::Audit => self.audit(ui),
                        }
                        ui.add_space(28.0);
                    });
            });
    }
    fn header(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                kit::heading(ui, self.page.title());
                kit::label(ui, self.page.subtitle(), 13.0, Weight::Regular, p.text_2);
                if self.active_scan.is_some() {
                    kit::badge_icon(
                        ui,
                        Some(icons::BROADCAST),
                        if self.rescanning {
                            "Rescanning · saved index shown until it finishes"
                        } else {
                            "Scanning · results are live"
                        },
                        p.teal,
                    );
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let scan = Button::primary("Scan a location")
                    .icon(icons::SCAN)
                    .enabled(!self.busy)
                    .show(ui);
                if kit::shortcut_tooltip(scan, "Choose a folder to index", &["⌘", "O"]).clicked()
                {
                    self.show_scan_dialog = true;
                }
                let refresh =
                    kit::icon_button_ex(ui, icons::ARROWS_CLOCKWISE, "", true, 36.0, None);
                if kit::shortcut_tooltip(refresh, "Refresh", &["⌘", "R"]).clicked() {
                    self.refresh();
                }
                if self.queries.loading() {
                    kit::spinner(ui, 18.0, p.accent);
                }
            });
        });
    }
    pub(super) fn scan_modal(&mut self, ctx: &egui::Context) {
        if !self.show_scan_dialog {
            return;
        }
        let p = self.palette;
        let frame = Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(18.0)
            .inner_margin(24.0)
            .shadow(epaint::Shadow {
                offset: [0, 18],
                blur: 52,
                spread: 0,
                color: p.shadow,
            });
        let mut open: Option<String> = None;
        let mut rescan = false;
        let modal = egui::Modal::new(Id::new("scan-modal"))
            .frame(frame)
            .backdrop_color(Color32::from_black_alpha(if p.dark { 150 } else { 70 }))
            .show(ctx, |ui| {
                ui.set_width(580.0);
                ui.spacing_mut().item_spacing.y = 10.0;
                ui.horizontal(|ui| {
                    kit::icon_tile(ui, icons::SCAN, p.accent, 42.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        kit::label(ui, "Choose a scan location", 20.0, Weight::SemiBold, p.text);
                        kit::label(
                            ui,
                            "Start focused. Expand when you need to.",
                            13.0,
                            Weight::Regular,
                            p.text_2,
                        );
                    });
                });
                kit::label(
                    ui,
                    "Scanning reads metadata and saves a private local index. It never authorizes cleanup. Previous exclusions are preserved when rescanning a known root.",
                    13.0,
                    Weight::Regular,
                    p.text_2,
                );
                ui.add_space(4.0);
                let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
                let picks: [(&str, &str, Option<&str>, Color32); 6] = [
                    ("Downloads", icons::DOWNLOAD_SIMPLE, Some("Downloads"), p.blue),
                    ("Projects", icons::CODE, Some("Projects"), p.purple),
                    ("Desktop", icons::DESKTOP, Some("Desktop"), p.cyan),
                    ("Documents", icons::FILE_TEXT, Some("Documents"), p.teal),
                    ("Home", icons::HOUSE, Some(""), p.amber),
                    ("Choose folder…", icons::FOLDER_OPEN, None, p.accent),
                ];
                let mut chosen: Option<Option<String>> = None;
                let root = self.root.clone();
                ui.columns(3, |columns| {
                    for (index, (label, icon, suffix, color)) in picks.iter().enumerate() {
                        let target = suffix.map(|s| {
                            if s.is_empty() {
                                home.display().to_string()
                            } else {
                                home.join(s).display().to_string()
                            }
                        });
                        let selected = target.as_deref() == Some(root.as_str());
                        let ui = &mut columns[index % 3];
                        if pick_tile(ui, icon, label, *color, selected).clicked() {
                            chosen = Some(target.clone());
                        }
                    }
                });
                match chosen {
                    Some(Some(path)) => self.root = path,
                    Some(None) => {
                        if let Some(folder) = rfd::FileDialog::new()
                            .set_title("Choose a folder to index")
                            .pick_folder()
                        {
                            self.root = folder.display().to_string();
                        }
                    }
                    None => {}
                }
                ui.add_space(2.0);
                kit::eyebrow(ui, "Folder to index");
                let width = ui.available_width();
                kit::text_field(
                    ui,
                    &mut self.root,
                    "Absolute folder path",
                    Some(icons::FOLDER_SIMPLE),
                    width,
                    true,
                );
                if !self.indexed_roots.is_empty() {
                    kit::eyebrow(ui, "Saved indexes");
                    kit::caption(
                        ui,
                        "Open one instantly from its saved index. Rescan only when you want fresh numbers.",
                    );
                    for root in self.indexed_roots.clone() {
                        ui.horizontal(|ui| {
                            if Button::soft(truncate_middle(&short_path(&root), 50), p.teal)
                                .icon(icons::FOLDER_OPEN)
                                .small()
                                .show(ui)
                                .on_hover_text("Open the saved index without scanning")
                                .clicked()
                            {
                                open = Some(root.clone());
                            }
                            if Button::ghost("Rescan")
                                .icon(icons::ARROWS_CLOCKWISE)
                                .small()
                                .show(ui)
                                .on_hover_text(
                                    "Index this folder again; the saved index stays until it finishes",
                                )
                                .clicked()
                            {
                                self.root = root.clone();
                                rescan = true;
                            }
                        });
                    }
                }
                kit::caption(
                    ui,
                    "macOS may restrict some folders; warnings are recorded and no administrator privileges are requested. Overlapping roots are rejected to prevent double counting.",
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if Button::primary("Start read-only scan")
                            .icon(icons::PLAY)
                            .min_width(200.0)
                            .enabled(!self.busy && !self.root.trim().is_empty())
                            .show(ui)
                            .clicked()
                        {
                            self.start_scan();
                        }
                        if Button::ghost("Cancel").show(ui).clicked() {
                            self.show_scan_dialog = false;
                        }
                    });
                });
            });
        if modal.should_close() {
            self.show_scan_dialog = false;
        }
        if let Some(root) = open {
            self.open_indexed(root);
        } else if rescan {
            self.start_scan();
        }
    }
    pub(super) fn activity_card(&mut self, ctx: &egui::Context) {
        if !self.busy {
            return;
        }
        let p = self.palette;
        egui::Area::new(Id::new("activity"))
            .order(Order::Foreground)
            .anchor(Align2::RIGHT_BOTTOM, vec2(-24.0, -24.0))
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_width(340.0);
                Card::new().elevated().padding(14.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.horizontal(|ui| {
                        kit::spinner(ui, 22.0, p.accent);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            let title = match self.activity {
                                Activity::Scan if self.paused => "Scan paused",
                                Activity::Scan => "Scanning read-only",
                                Activity::Duplicates => "Verifying duplicate content",
                                Activity::Cleanup => "Quarantine in progress",
                                Activity::Generic => "Working locally",
                            };
                            kit::label(ui, title, 13.5, Weight::SemiBold, p.text);
                            let status = kit::galley_truncated(
                                ui,
                                &self.status,
                                fonts::font(12.0, Weight::Regular),
                                p.text_2,
                                260.0,
                            );
                            let (rect, _) = ui.allocate_exact_size(
                                vec2(status.size().x, status.size().y),
                                Sense::hover(),
                            );
                            ui.painter().galley(rect.min, status, p.text_2);
                        });
                    });
                    if self.activity == Activity::Scan {
                        indeterminate_bar(ui, p.accent, !self.paused);
                        kit::caption(
                            ui,
                            if self.rescanning {
                                "Your saved index stays available until this scan finishes."
                            } else {
                                "Results appear on every page as the scan runs."
                            },
                        );
                        ui.horizontal(|ui| {
                            if let Some(scan) = self.active_scan.clone() {
                                let (label, icon) = if self.paused {
                                    ("Resume", icons::PLAY)
                                } else {
                                    ("Pause", icons::PAUSE)
                                };
                                if Button::new(label).icon(icon).small().show(ui).clicked() {
                                    match self.engine.control_scan(
                                        &scan,
                                        if self.paused { "resume" } else { "pause" },
                                    ) {
                                        Ok(()) => self.paused = !self.paused,
                                        Err(e) => self.error = Some(e.to_string()),
                                    }
                                }
                                if Button::soft("Cancel scan", p.rose)
                                    .icon(icons::X)
                                    .small()
                                    .show(ui)
                                    .clicked()
                                {
                                    self.engine.cancel_all();
                                }
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if self.scan_entries > 0 {
                                    kit::caption(
                                        ui,
                                        format!("{} entries", util::count(self.scan_entries)),
                                    );
                                }
                            });
                        });
                    } else if self.activity == Activity::Duplicates {
                        indeterminate_bar(ui, p.purple, true);
                        if Button::soft("Cancel hashing", p.rose)
                            .icon(icons::X)
                            .small()
                            .show(ui)
                            .clicked()
                        {
                            self.duplicate_cancel
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                });
            });
    }
}

/// A quick-pick location tile used by the scan modal.
fn pick_tile(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    color: Color32,
    selected: bool,
) -> egui::Response {
    let p = kit::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 74.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let h = ui
        .ctx()
        .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    let s = ui
        .ctx()
        .animate_bool_with_time(response.id.with("selected"), selected, 0.16);
    let painter = ui.painter();
    let fill = p.sunken.lerp_to_gamma(p.raised, h * 0.7);
    let stroke = p
        .border
        .lerp_to_gamma(p.accent, s)
        .lerp_to_gamma(p.border_strong, h * (1.0 - s));
    painter.rect(
        rect,
        12.0,
        fill,
        Stroke::new(1.0 + s, stroke),
        StrokeKind::Inside,
    );
    if s > 0.0 {
        painter.rect_filled(rect, 12.0, p.accent.gamma_multiply(0.08 * s));
    }
    let tile = Rect::from_center_size(pos2(rect.center().x, rect.top() + 28.0), Vec2::splat(28.0));
    kit::paint_icon_tile(painter, tile, icon, color);
    painter.text(
        pos2(rect.center().x, rect.bottom() - 15.0),
        Align2::CENTER_CENTER,
        label,
        fonts::font(12.5, Weight::Medium),
        p.text,
    );
    response
}

/// A gliding highlight for work without a known total.
fn indeterminate_bar(ui: &mut egui::Ui, color: Color32, animate: bool) {
    let p = kit::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, p.wash(0.08));
    if animate {
        ui.ctx().request_repaint();
        let time = ui.input(|i| i.time) as f32;
        let phase = (time * 0.9) % 1.0;
        let width = rect.width() * 0.32;
        let x = rect.left() - width + phase * (rect.width() + width);
        let segment = Rect::from_min_size(pos2(x, rect.top()), vec2(width, rect.height()));
        painter
            .with_clip_rect(rect)
            .rect_filled(segment, 3.0, color);
    } else {
        painter.rect_filled(rect, 3.0, color.gamma_multiply(0.45));
    }
}
