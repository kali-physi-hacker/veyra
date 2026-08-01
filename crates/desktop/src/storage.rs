use super::*;
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
    pub(super) fn navigation(&mut self, ui: &mut egui::Ui) {
        let mut target = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.nav_back.is_empty(), egui::Button::new("Back"))
                .clicked()
            {
                target = self.nav_back.pop();
                self.nav_forward.push(self.path.clone());
            }
            if ui
                .add_enabled(!self.nav_forward.is_empty(), egui::Button::new("Forward"))
                .clicked()
            {
                target = self.nav_forward.pop();
                self.nav_back.push(self.path.clone());
            }
            egui::ComboBox::from_id_salt("indexed-locations")
                .selected_text("Locations")
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
            let ancestors: Vec<_> = std::path::Path::new(&self.path).ancestors().collect();
            for ancestor in ancestors.iter().take(5).rev() {
                ui.label(RichText::new("/").color(MUTED));
                let name = ancestor
                    .file_name()
                    .map_or_else(|| "Root".into(), |s| s.to_string_lossy());
                if ui
                    .small_button(truncate(&name, 22))
                    .on_hover_text(ancestor.display().to_string())
                    .clicked()
                {
                    target = Some(ancestor.display().to_string());
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
        ui.add_space(8.0);
    }
    pub(super) fn explorer(&mut self, ui: &mut egui::Ui) {
        if self.path.is_empty() {
            empty(
                ui,
                "Choose an indexed location",
                "Scan a folder first, then browse its saved index here.",
            );
            return;
        }
        self.navigation(ui);
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in [
                (FileMode::Children, "This folder"),
                (FileMode::Largest, "Largest files"),
                (FileMode::Recent, "Modified this week"),
            ] {
                if ui.selectable_label(self.file_mode == mode, label).clicked() {
                    self.file_mode = mode;
                    changed = true;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Filter names, e.g. *.zip")
                    .desired_width(240.0),
            );
            egui::ComboBox::from_id_salt("file-sort")
                .selected_text(match self.sort.as_str() {
                    "allocated_bytes" => "Allocated size",
                    "modified_at" => "Last modified",
                    "path" => "Path",
                    _ => "Logical size",
                })
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
            if ui.button("Apply").clicked() {
                changed = true;
            }
        });
        if changed {
            self.offset = 0;
            self.files.clear();
            self.refresh();
        }
        ui.add_space(6.0);
        if self.files.is_empty() && !self.queries.loading() {
            empty(
                ui,
                "No matching indexed entries",
                "Try another filter or location. Files outside your scan scope are not included.",
            );
        }
        let mut selected = None;
        let mut navigate = None;
        let name_width = (ui.available_width() - 330.0).max(180.0);
        egui::Grid::new("explorer-rows")
            .striped(true)
            .num_columns(4)
            .spacing([18.0, 12.0])
            .show(ui, |ui| {
                for label in ["NAME", "LOGICAL", "ALLOCATED", "TYPE"] {
                    eyebrow(ui, label);
                }
                ui.end_row();
                for entry in &self.files {
                    let label = if entry.kind == EntryKind::Directory {
                        format!("{} /", entry.name)
                    } else {
                        entry.name.clone()
                    };
                    let response = ui
                        .add_sized(
                            [name_width, 28.0],
                            egui::Button::new(
                                RichText::new(truncate(&label, (name_width / 8.0) as usize)).color(
                                    if entry.kind == EntryKind::Directory {
                                        ACCENT
                                    } else {
                                        Color32::from_rgb(224, 228, 239)
                                    },
                                ),
                            )
                            .selected(
                                self.selected_entry
                                    .as_ref()
                                    .is_some_and(|e| e.path == entry.path),
                            )
                            .frame(false),
                        )
                        .on_hover_text(&entry.path);
                    if response.clicked() {
                        selected = Some(entry.clone());
                    }
                    if response.double_clicked() && entry.kind == EntryKind::Directory {
                        navigate = Some(entry.path.clone());
                    }
                    ui.label(bytes(entry.logical_bytes));
                    ui.label(RichText::new(bytes(entry.allocated_bytes)).color(MUTED));
                    ui.label(entry.kind.as_str());
                    ui.end_row();
                }
            });
        if let Some(entry) = selected {
            self.selected_entry = Some(entry);
        }
        if let Some(path) = navigate {
            self.browse(path);
        }
        self.pager(ui);
        self.inspector(ui);
    }
    pub(super) fn pager(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.offset >= 100, egui::Button::new("Previous"))
                .clicked()
            {
                self.offset -= 100;
                self.refresh();
            }
            ui.label(
                RichText::new(format!(
                    "Page {} · up to 100 indexed entries",
                    self.offset / 100 + 1
                ))
                .small()
                .color(MUTED),
            );
            if ui
                .add_enabled(self.has_more, egui::Button::new("Next"))
                .clicked()
            {
                self.offset += 100;
                self.refresh();
            }
        });
    }
    pub(super) fn map(&mut self, ui: &mut egui::Ui) {
        if self.path.is_empty() {
            empty(
                ui,
                "Your map starts with a scan",
                "Choose a folder. Its files and directories will both appear in the map.",
            );
            return;
        }
        self.navigation(ui);
        let Some(breakdown) = self.breakdown.clone() else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(bytes(breakdown.children_logical_bytes))
                    .size(26.0)
                    .strong(),
            );
            ui.label(
                RichText::new(format!(
                    "in {} direct children · logical sizes",
                    breakdown.child_count
                ))
                .color(MUTED),
            );
            pill(
                ui,
                "Click to inspect · double-click folders to open",
                ACCENT,
            );
        });
        if breakdown.child_count == 0 {
            empty(
                ui,
                "This indexed directory is empty",
                "No immediate children were recorded. Permission warnings and scan exclusions can also limit what was indexed.",
            );
            return;
        }
        let mut weights: Vec<_> = breakdown.children.iter().map(|e| e.logical_bytes).collect();
        if breakdown.omitted_count > 0 {
            weights.push(breakdown.omitted_logical_bytes);
        }
        let width = ui.available_width();
        let mut selected = None;
        let mut navigate = None;
        let mut remainder = false;
        let mut hovered = None;
        ui.horizontal_top(|ui| {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new((width * 0.64).max(300.0), 420.0),
                egui::Sense::hover(),
            );
            ui.painter()
                .rect_filled(rect, 12.0, Color32::from_rgb(18, 22, 33));
            let tiles = tile_layout(&weights, rect);
            for (index, tile) in tiles {
                let entry = breakdown.children.get(index);
                let is_remainder = entry.is_none();
                let key = entry.map_or("__remainder", |e| e.path.as_str());
                let response =
                    ui.interact(tile.shrink(2.0), ui.id().with(key), egui::Sense::click());
                let highlighted = response.hovered()
                    || self.hovered_path.as_deref() == Some(key)
                    || self.selected_entry.as_ref().is_some_and(|e| e.path == key);
                let color = if is_remainder {
                    MUTED
                } else {
                    COLORS[index % 6]
                };
                ui.painter().rect_filled(
                    tile.shrink(2.0),
                    6.0,
                    color.gamma_multiply(if highlighted { 0.55 } else { 0.38 }),
                );
                if highlighted {
                    ui.painter().rect_stroke(
                        tile.shrink(2.0),
                        6.0,
                        egui::Stroke::new(2.0, color),
                        egui::StrokeKind::Inside,
                    );
                }
                let name = entry.map_or_else(
                    || format!("{} other items", breakdown.omitted_count),
                    |e| e.name.clone(),
                );
                if tile.width() > 85.0 && tile.height() > 50.0 {
                    ui.painter().text(
                        tile.min + Vec2::splat(11.0),
                        egui::Align2::LEFT_TOP,
                        format!(
                            "{}\n{}",
                            truncate(&name, ((tile.width() - 22.0) / 8.0) as usize),
                            bytes(weights[index])
                        ),
                        egui::FontId::proportional(13.0),
                        Color32::WHITE,
                    );
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
            ui.vertical(|ui| {
                ui.set_width((width * 0.36 - 18.0).max(180.0));
                eyebrow(ui, "Largest contributions");
                egui::ScrollArea::vertical()
                    .id_salt("map-list")
                    .max_height(385.0)
                    .show(ui, |ui| {
                        for (index, entry) in breakdown.children.iter().enumerate() {
                            let chosen = self
                                .selected_entry
                                .as_ref()
                                .is_some_and(|e| e.path == entry.path);
                            let response = ui.add_sized(
                                [ui.available_width(), 46.0],
                                egui::Button::new(
                                    RichText::new(format!(
                                        "{}{}\n{}",
                                        truncate(&entry.name, 27),
                                        if entry.kind == EntryKind::Directory {
                                            " /"
                                        } else {
                                            ""
                                        },
                                        bytes(entry.logical_bytes)
                                    ))
                                    .color(COLORS[index % 6]),
                                )
                                .selected(chosen)
                                .frame(chosen),
                            );
                            if response.hovered() {
                                hovered = Some(entry.path.clone());
                            }
                            if response.clicked() {
                                selected = Some(entry.clone());
                            }
                            if response.double_clicked() && entry.kind == EntryKind::Directory {
                                navigate = Some(entry.path.clone());
                            }
                        }
                        if breakdown.omitted_count > 0
                            && ui
                                .button(format!(
                                    "{} other items\n{} · view all",
                                    breakdown.omitted_count,
                                    bytes(breakdown.omitted_logical_bytes)
                                ))
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
        ui.label(RichText::new("Files and folders share the same scale. Zero-byte entries remain in the list. Physical storage may differ because of sparse files, links and APFS clones.").small().color(MUTED));
        self.inspector(ui);
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        let Some(entry) = self.selected_entry.clone() else {
            ui.add_space(10.0);
            ui.label(
                RichText::new("Select an item to see its details and classification evidence.")
                    .color(MUTED),
            );
            return;
        };
        ui.add_space(12.0);
        card().show(ui,|ui|{
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui|{eyebrow(ui,"Selected item");pill(ui,entry.kind.as_str(),ACCENT);ui.strong(&entry.name);});
            ui.label(RichText::new(short_path(&entry.path)).monospace().small()).on_hover_text(&entry.path);
            ui.horizontal_wrapped(|ui|{ui.label(format!("{} logical",bytes(entry.logical_bytes)));ui.label(format!("{} allocated",bytes(entry.allocated_bytes)));ui.label(format!("{} · {:.0}% classification confidence",entry.category.replace('_'," "),entry.confidence*100.0));});
            if let Some(modified)=entry.modified_at{ui.label(RichText::new(format!("Modified {} · filesystem link count {}",age(modified),entry.identity.links)).small().color(MUTED));}
            ui.horizontal_wrapped(|ui|{
                if entry.kind==EntryKind::Directory && ui.button("Open folder").clicked(){self.browse(entry.path.clone());}
                if ui.button("Reveal in Finder").clicked(){self.reveal(&entry.path);}
                if ui.button("Copy path").clicked(){ui.ctx().copy_text(entry.path.clone());}
                if ui.button("Close details").clicked(){self.selected_entry=None;}
            });
            egui::CollapsingHeader::new("Classification evidence").id_salt(&entry.path).show(ui,|ui|{for evidence in &entry.evidence{ui.label(format!("{}: {}",evidence.mechanism,evidence.detail));}ui.label("Large or old does not mean expendable. No cleanup action is authorized by this inspector.");});
        });
    }
}
fn tile_layout(weights: &[u64], rect: egui::Rect) -> Vec<(usize, egui::Rect)> {
    fn split(items: &[(usize, u64)], rect: egui::Rect, out: &mut Vec<(usize, egui::Rect)>) {
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
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 500.0));
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
