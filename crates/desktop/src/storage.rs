use super::*;
use eframe::egui::{Align, Align2, CursorIcon, Layout, Rect, Sense, Stroke, StrokeKind, vec2};
use kit::{Button, Card, Row};

impl App {
    pub(super) fn browse(&mut self, path: String) {
        if self.path != path && !self.path.is_empty() {
            self.nav_back.push(self.path.clone());
            self.nav_forward.clear();
        }
        self.path = path;
        self.offset = 0;
        self.file_mode = FileMode::Children;
        self.files.clear();
        self.breakdown = None;
        self.selected_entry = None;
        self.hovered_path = None;
        self.refresh();
    }
    /// Switch to `page` and browse `path` there in one step.
    pub(super) fn open_location(&mut self, page: Page, path: String) {
        if self.page != page {
            self.page = page;
            self.page_entered = Instant::now();
            self.offset = 0;
            self.has_more = false;
            self.error = None;
        }
        self.browse(path);
    }
    pub(super) fn navigation(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let mut target = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if kit::icon_button_ex(
                ui,
                icons::ARROW_LEFT,
                "Back",
                !self.nav_back.is_empty(),
                32.0,
                None,
            )
            .clicked()
            {
                target = self.nav_back.pop();
                self.nav_forward.push(self.path.clone());
            }
            if kit::icon_button_ex(
                ui,
                icons::ARROW_RIGHT,
                "Forward",
                !self.nav_forward.is_empty(),
                32.0,
                None,
            )
            .clicked()
            {
                target = self.nav_forward.pop();
                self.nav_back.push(self.path.clone());
            }
            ui.add_space(6.0);
            egui::ComboBox::from_id_salt("indexed-locations")
                .selected_text(fonts::text("Locations", 13.0, Weight::Medium, p.text))
                .width(132.0)
                .show_ui(ui, |ui| {
                    for root in &self.indexed_roots {
                        if ui
                            .selectable_label(self.path == *root, short_path(root))
                            .clicked()
                        {
                            target = Some(root.clone());
                        }
                    }
                });
            ui.add_space(8.0);
            let ancestors: Vec<String> = std::path::Path::new(&self.path)
                .ancestors()
                .take(5)
                .map(|a| a.display().to_string())
                .collect();
            let count = ancestors.len();
            for (index, ancestor) in ancestors.iter().rev().enumerate() {
                if index > 0 {
                    kit::icon(ui, icons::CARET_RIGHT, 11.0, p.text_3);
                }
                let name = std::path::Path::new(ancestor)
                    .file_name()
                    .map_or_else(|| "Root".to_string(), |s| s.to_string_lossy().into_owned());
                let last = index + 1 == count;
                let button = if last {
                    Button::soft(truncate(&name, 26), p.accent).small()
                } else {
                    Button::ghost(truncate(&name, 22)).small()
                };
                if button.show(ui).on_hover_text(ancestor).clicked() && !last {
                    target = Some(ancestor.clone());
                }
            }
        });
        if let Some(target) = target {
            self.path = target;
            self.offset = 0;
            self.file_mode = FileMode::Children;
            self.breakdown = None;
            self.files.clear();
            self.selected_entry = None;
            self.refresh();
        }
        ui.add_space(4.0);
    }
    pub(super) fn explorer(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        if self.path.is_empty() {
            kit::empty_state(
                ui,
                icons::FOLDER_OPEN,
                "Choose an indexed location",
                "Scan a folder first, then browse its saved index here.",
            );
            return;
        }
        self.navigation(ui);
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= kit::segmented(
                ui,
                egui::Id::new("file-mode"),
                &mut self.file_mode,
                &[
                    (FileMode::Children, "This folder"),
                    (FileMode::Largest, "Largest files"),
                    (FileMode::Recent, "Modified this week"),
                ],
            );
            ui.add_space(6.0);
            if kit::search_field(ui, &mut self.search, "Filter names, e.g. *.zip", 250.0).changed()
            {
                changed = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                egui::ComboBox::from_id_salt("file-sort")
                    .selected_text(fonts::text(
                        match self.sort.as_str() {
                            "allocated_bytes" => "Allocated size",
                            "modified_at" => "Last modified",
                            "path" => "Path",
                            _ => "Logical size",
                        },
                        13.0,
                        Weight::Medium,
                        p.text,
                    ))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            ("logical_bytes", "Logical size"),
                            ("allocated_bytes", "Allocated size"),
                            ("modified_at", "Last modified"),
                            ("path", "Path"),
                        ] {
                            if ui
                                .selectable_value(&mut self.sort, value.into(), label)
                                .changed()
                            {
                                changed = true;
                            }
                        }
                    });
                kit::caption(ui, "Sort by");
            });
        });
        if changed {
            self.offset = 0;
            self.files.clear();
            self.refresh();
        }
        ui.add_space(2.0);
        if self.files.is_empty() {
            if self.queries.waiting() {
                kit::skeleton(ui, 6);
            } else {
                kit::empty_state(
                    ui,
                    icons::FUNNEL,
                    "No matching indexed entries",
                    "Try another filter or location. Files outside your scan scope are not included.",
                );
            }
        } else {
            let mut selected = None;
            let mut navigate = None;
            Card::new().padding(8.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    kit::eyebrow(ui, "Name");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(12.0);
                        kit::eyebrow(ui, "Logical size");
                    });
                });
                ui.add_space(2.0);
                for entry in &self.files {
                    let is_dir = entry.kind == EntryKind::Directory;
                    let (icon, color) = if is_dir {
                        (icons::FOLDER_SIMPLE, p.teal)
                    } else {
                        kit::category_style(&p, &entry.category)
                    };
                    let mut subtitle = format!(
                        "{} · {} allocated",
                        if is_dir {
                            "Folder".to_string()
                        } else {
                            humanize(&entry.category)
                        },
                        bytes(entry.allocated_bytes)
                    );
                    if let Some(modified) = entry.modified_at {
                        subtitle.push_str(&format!(" · modified {}", age(modified)));
                    }
                    let chosen = self
                        .selected_entry
                        .as_ref()
                        .is_some_and(|e| e.path == entry.path);
                    let mut row = Row::new(&entry.name)
                        .icon(icon, color)
                        .subtitle(subtitle)
                        .trailing(bytes(entry.logical_bytes), p.text)
                        .selected(chosen);
                    if is_dir {
                        row = row.chevron();
                    } else if entry.kind != EntryKind::File {
                        row = row.badge(entry.kind.as_str(), p.text_3);
                    }
                    let response = row.show(ui).on_hover_text(&entry.path);
                    if response.clicked() {
                        selected = Some(entry.clone());
                    }
                    if response.double_clicked() && is_dir {
                        navigate = Some(entry.path.clone());
                    }
                }
            });
            if let Some(entry) = selected {
                self.selected_entry = Some(entry);
            }
            if let Some(path) = navigate {
                self.browse(path);
            }
        }
        self.pager(ui);
        self.inspector(ui);
    }
    pub(super) fn pager(&mut self, ui: &mut egui::Ui) {
        if self.offset < 100 && !self.has_more {
            return;
        }
        ui.horizontal(|ui| {
            if Button::ghost("Previous")
                .icon(icons::ARROW_LEFT)
                .small()
                .enabled(self.offset >= 100)
                .show(ui)
                .clicked()
            {
                self.offset -= 100;
                self.refresh();
            }
            kit::caption(
                ui,
                format!("Page {} · up to 100 indexed entries", self.offset / 100 + 1),
            );
            if Button::ghost("Next")
                .icon(icons::ARROW_RIGHT)
                .trailing()
                .small()
                .enabled(self.has_more)
                .show(ui)
                .clicked()
            {
                self.offset += 100;
                self.refresh();
            }
        });
    }
    pub(super) fn map(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        if self.path.is_empty() {
            kit::empty_state(
                ui,
                icons::SQUARES_FOUR,
                "Your map starts with a scan",
                "Choose a folder. Its files and directories will both appear in the map.",
            );
            return;
        }
        self.navigation(ui);
        let Some(breakdown) = self.breakdown.clone() else {
            if self.queries.waiting() {
                kit::skeleton(ui, 6);
            }
            return;
        };
        ui.horizontal(|ui| {
            kit::label(
                ui,
                bytes(breakdown.children_logical_bytes),
                26.0,
                Weight::Bold,
                p.text,
            );
            kit::label(
                ui,
                format!(
                    "in {} direct children · logical sizes",
                    util::count(breakdown.child_count)
                ),
                13.0,
                Weight::Regular,
                p.text_2,
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                kit::badge_icon(
                    ui,
                    Some(icons::HAND_PALM),
                    "Click to inspect · double-click folders to open",
                    p.accent,
                );
            });
        });
        if breakdown.child_count == 0 {
            kit::empty_state(
                ui,
                icons::FOLDER_DASHED,
                "This indexed directory is empty",
                "No immediate children were recorded. Permission warnings and scan exclusions can also limit what was indexed.",
            );
            return;
        }
        let mut weights: Vec<_> = breakdown.children.iter().map(|e| e.logical_bytes).collect();
        if breakdown.omitted_count > 0 {
            weights.push(breakdown.omitted_logical_bytes);
        }
        let t = self.reveal;
        let width = ui.available_width();
        let list_width = 300.0;
        let map_width = (width - list_width - 16.0).max(320.0);
        let map_height = 440.0;
        let mut selected = None;
        let mut navigate = None;
        let mut remainder = false;
        let mut hovered = None;
        ui.horizontal_top(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(map_width, map_height), Sense::hover());
            ui.painter().rect(
                rect,
                14.0,
                p.sunken,
                Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
            let tiles = tile_layout(&weights, rect.shrink(5.0));
            for (index, tile) in tiles {
                let entry = breakdown.children.get(index);
                let is_remainder = entry.is_none();
                let key = entry.map_or("__remainder", |e| e.path.as_str());
                let inner = tile.shrink(2.5);
                let response = ui
                    .interact(inner, ui.id().with(key), Sense::click())
                    .on_hover_cursor(CursorIcon::PointingHand);
                let is_selected = self.selected_entry.as_ref().is_some_and(|e| e.path == key);
                let is_hovered = response.hovered() || self.hovered_path.as_deref() == Some(key);
                let h = ui.ctx().animate_bool_with_time(
                    response.id.with("tile"),
                    is_hovered || is_selected,
                    0.14,
                );
                let color = if is_remainder {
                    p.text_3
                } else {
                    p.series(index)
                };
                let drawn =
                    Rect::from_center_size(inner.center(), inner.size() * (0.86 + 0.14 * t));
                let painter = ui.painter();
                painter.rect_filled(
                    drawn,
                    8.0,
                    color.gamma_multiply((0.32 + 0.28 * h) * (0.35 + 0.65 * t)),
                );
                painter.hline(
                    (drawn.left() + 8.0)..=(drawn.right() - 8.0),
                    drawn.top() + 1.5,
                    Stroke::new(1.0, Color32::from_white_alpha((28.0 + 30.0 * h) as u8)),
                );
                painter.rect_stroke(
                    drawn,
                    8.0,
                    Stroke::new(1.0 + h, color.gamma_multiply(0.5 + 0.5 * h)),
                    StrokeKind::Inside,
                );
                let name = entry.map_or_else(
                    || format!("{} other items", breakdown.omitted_count),
                    |e| e.name.clone(),
                );
                if drawn.width() > 92.0 && drawn.height() > 46.0 {
                    let glyph = match entry {
                        Some(e) if e.kind == EntryKind::Directory => icons::FOLDER_SIMPLE,
                        Some(_) => icons::FILE,
                        None => icons::DOTS_THREE,
                    };
                    kit::paint_icon(
                        painter,
                        drawn.min + vec2(17.0, 16.0),
                        glyph,
                        14.0,
                        Color32::from_white_alpha(230),
                        false,
                    );
                    let label = kit::galley_truncated(
                        ui,
                        &name,
                        fonts::font(13.0, Weight::Medium),
                        Color32::WHITE,
                        drawn.width() - 40.0,
                    );
                    painter.galley(drawn.min + vec2(28.0, 8.0), label, Color32::WHITE);
                    painter.text(
                        drawn.min + vec2(11.0, 30.0),
                        Align2::LEFT_TOP,
                        bytes(weights[index]),
                        fonts::font(12.0, Weight::Regular),
                        Color32::from_white_alpha(200),
                    );
                    if drawn.height() > 76.0 {
                        painter.text(
                            drawn.min + vec2(11.0, 48.0),
                            Align2::LEFT_TOP,
                            format!(
                                "{:.1}%",
                                weights[index] as f64
                                    / breakdown.children_logical_bytes.max(1) as f64
                                    * 100.0
                            ),
                            fonts::font(11.0, Weight::Medium),
                            Color32::from_white_alpha(150),
                        );
                    }
                }
                if response.hovered() {
                    hovered = Some(key.to_string());
                }
                if response.clicked() {
                    if let Some(e) = entry {
                        selected = Some(e.clone());
                    } else {
                        remainder = true;
                    }
                }
                if response.double_clicked()
                    && let Some(e) = entry
                    && e.kind == EntryKind::Directory
                {
                    navigate = Some(e.path.clone());
                }
                response.on_hover_text(format!(
                    "{name}\n{} · {:.1}% of immediate-child logical bytes",
                    bytes(weights[index]),
                    weights[index] as f64 / breakdown.children_logical_bytes.max(1) as f64 * 100.0
                ));
            }
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.set_width(list_width);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    kit::eyebrow(ui, "Largest contributions");
                });
                egui::ScrollArea::vertical()
                    .id_salt("map-list")
                    .max_height(map_height - 26.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (index, entry) in breakdown.children.iter().enumerate() {
                            let chosen = self
                                .selected_entry
                                .as_ref()
                                .is_some_and(|e| e.path == entry.path);
                            let is_dir = entry.kind == EntryKind::Directory;
                            let mut row = Row::new(&entry.name)
                                .swatch(p.series(index))
                                .trailing(bytes(entry.logical_bytes), p.text)
                                .height(40.0)
                                .selected(chosen);
                            if is_dir {
                                row = row.chevron();
                            }
                            let response = row.show(ui);
                            if response.hovered() {
                                hovered = Some(entry.path.clone());
                            }
                            if response.clicked() {
                                selected = Some(entry.clone());
                            }
                            if response.double_clicked() && is_dir {
                                navigate = Some(entry.path.clone());
                            }
                        }
                        if breakdown.omitted_count > 0
                            && Row::new(format!("{} other items", breakdown.omitted_count))
                                .swatch(p.text_3)
                                .subtitle("View all in the explorer")
                                .trailing(bytes(breakdown.omitted_logical_bytes), p.text_2)
                                .height(46.0)
                                .chevron()
                                .show(ui)
                                .clicked()
                        {
                            remainder = true;
                        }
                    });
            });
        });
        self.hovered_path = hovered;
        if let Some(entry) = selected {
            self.selected_entry = Some(entry);
        }
        if let Some(path) = navigate {
            self.browse(path);
        }
        if remainder {
            self.choose_page(Page::Explorer);
        }
        kit::caption(
            ui,
            "Files and folders share the same scale. Zero-byte entries remain in the list. Physical storage may differ because of sparse files, links and APFS clones.",
        );
        self.inspector(ui);
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let Some(entry) = self.selected_entry.clone() else {
            ui.add_space(2.0);
            kit::caption(
                ui,
                "Select an item to see its details and classification evidence.",
            );
            return;
        };
        let is_dir = entry.kind == EntryKind::Directory;
        let (icon, color) = if is_dir {
            (icons::FOLDER_SIMPLE, p.teal)
        } else {
            kit::category_style(&p, &entry.category)
        };
        let mut close = false;
        let mut open = None;
        let mut reveal = None;
        let mut history = None;
        Card::new().padding(18.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                kit::icon_tile(ui, icon, color, 42.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.horizontal(|ui| {
                        kit::label(ui, &entry.name, 16.0, Weight::SemiBold, p.text);
                        kit::badge(ui, entry.kind.as_str(), p.accent);
                        kit::badge(ui, &humanize(&entry.category), color);
                    });
                    kit::mono(ui, short_path(&entry.path), 11.5)
                        .on_hover_text(&entry.path);
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if kit::icon_button(ui, icons::X, "Close details").clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(10.0);
            ui.columns(4, |cols| {
                stat(&mut cols[0], "Logical", &bytes(entry.logical_bytes));
                stat(&mut cols[1], "Allocated", &bytes(entry.allocated_bytes));
                stat(
                    &mut cols[2],
                    "Classification",
                    &format!("{:.0}% confidence", entry.confidence * 100.0),
                );
                stat(
                    &mut cols[3],
                    "Modified",
                    &entry.modified_at.map_or_else(|| "unknown".into(), age),
                );
            });
            kit::caption(
                ui,
                format!(
                    "Filesystem link count {} · device {} · inode {}",
                    entry.identity.links, entry.identity.device, entry.identity.inode
                ),
            );
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if is_dir
                    && Button::new("Open folder")
                        .icon(icons::FOLDER_OPEN)
                        .small()
                        .show(ui)
                        .clicked()
                {
                    open = Some(entry.path.clone());
                }
                if Button::new("Reveal in Finder")
                    .icon(icons::ARROW_SQUARE_OUT)
                    .small()
                    .show(ui)
                    .clicked()
                {
                    reveal = Some(entry.path.clone());
                }
                if Button::ghost("Copy path")
                    .icon(icons::COPY)
                    .small()
                    .show(ui)
                    .clicked()
                {
                    ui.ctx().copy_text(entry.path.clone());
                }
                if is_dir
                    && Button::ghost("Storage history")
                        .icon(icons::CHART_LINE_UP)
                        .small()
                        .show(ui)
                        .clicked()
                {
                    history = Some(entry.path.clone());
                }
            });
            kit::disclosure(ui, "Classification evidence", &entry.path, |ui| {
                for evidence in &entry.evidence {
                    kit::label(ui, humanize(&evidence.mechanism), 12.0, Weight::SemiBold, color);
                    kit::label(ui, &evidence.detail, 13.0, Weight::Regular, p.text_2);
                }
                kit::caption(
                    ui,
                    "Large or old does not mean expendable. No cleanup action is authorized by this inspector.",
                );
            });
        });
        if close {
            self.selected_entry = None;
        }
        if let Some(path) = open {
            self.browse(path);
        }
        if let Some(path) = reveal {
            self.reveal(&path);
        }
        if let Some(path) = history {
            self.open_location(Page::History, path);
        }
    }
}

fn stat(ui: &mut egui::Ui, label: &str, value: &str) {
    let p = kit::palette(ui.ctx());
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        kit::eyebrow(ui, label);
        kit::label(ui, value, 15.0, Weight::SemiBold, p.text);
    });
}

pub(super) fn tile_layout(weights: &[u64], rect: Rect) -> Vec<(usize, Rect)> {
    fn split(items: &[(usize, u64)], rect: Rect, out: &mut Vec<(usize, Rect)>) {
        if items.is_empty() {
            return;
        }
        if items.len() == 1 {
            out.push((items[0].0, rect));
            return;
        }
        let total = items.iter().map(|(_, n)| *n as f64).sum::<f64>();
        let mut sum = 0.0;
        let mut at = 1;
        for (i, (_, n)) in items.iter().take(items.len() - 1).enumerate() {
            sum += *n as f64;
            at = i + 1;
            if sum >= total / 2.0 {
                break;
            }
        }
        let ratio = (sum / total) as f32;
        let (mut a, mut b) = (rect, rect);
        if rect.width() >= rect.height() {
            a.max.x = rect.left() + rect.width() * ratio;
            b.min.x = a.max.x;
        } else {
            a.max.y = rect.top() + rect.height() * ratio;
            b.min.y = a.max.y;
        }
        split(&items[..at], a, out);
        split(&items[at..], b, out);
    }
    let items: Vec<_> = weights
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, n)| *n > 0)
        .collect();
    let mut output = Vec::new();
    split(&items, rect, &mut output);
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiles_preserve_area_and_do_not_overlap() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(800.0, 500.0));
        let weights = [70, 20, 5, 4, 1, 0];
        let tiles = tile_layout(&weights, rect);
        assert_eq!(tiles.len(), 5);
        for (i, (index, tile)) in tiles.iter().enumerate() {
            assert!((tile.area() / rect.area() - weights[*index] as f32 / 100.0).abs() < 0.0001);
            for (_, other) in &tiles[i + 1..] {
                let intersection = tile.intersect(*other);
                assert!(intersection.width() <= 0.0 || intersection.height() <= 0.0);
            }
        }
    }
}
