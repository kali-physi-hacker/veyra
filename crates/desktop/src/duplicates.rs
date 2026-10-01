use super::*;
use eframe::egui::{Align, Layout};
use kit::{Button, Card};

impl App {
    pub(super) fn duplicates(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        ui.horizontal(|ui| {
            if Button::primary("Verify duplicate content")
                .icon(icons::FINGERPRINT)
                .enabled(!self.busy)
                .show(ui)
                .clicked()
            {
                self.offset = 0;
                self.duplicate_running = true;
                self.duplicate_cancel
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                let cancel = self.duplicate_cancel.clone();
                self.task(Activity::Duplicates, move |e| {
                    Ok(Payload::Duplicates(e.discover_duplicates(&cancel)?))
                });
            }
            kit::caption(ui, "Size → sampled fingerprint → full BLAKE3 verification");
        });
        let mut reveal = None;
        if let Some(report) = self.duplicates.clone() {
            Card::new().padding(22.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    kit::icon_tile(ui, icons::COPY, p.purple, 44.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        kit::label(
                            ui,
                            format!("{} duplicate groups", util::count(report.group_count)),
                            24.0,
                            Weight::SemiBold,
                            p.text,
                        );
                        kit::label(
                            ui,
                            format!(
                                "{} files fully hashed · {} warnings · observed {}",
                                util::count(report.files_hashed),
                                report.warnings.len(),
                                age(report.analyzed_at)
                            ),
                            13.0,
                            Weight::Regular,
                            p.text_2,
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        if report.cancelled {
                            kit::badge_icon(ui, Some(icons::WARNING), "Cancelled early", p.amber);
                        }
                        if report.truncated {
                            kit::badge_icon(ui, Some(icons::DOTS_THREE), "Truncated", p.amber);
                        }
                        kit::badge_icon(ui, Some(icons::EYE), "Observation, not a live guarantee", p.text_3);
                    });
                });
                kit::caption(
                    ui,
                    "Sparse files and shared blocks affect potential physical savings. No copy is selected for deletion.",
                );
            });
            if report.groups.is_empty() {
                kit::empty_state(
                    ui,
                    icons::COPY,
                    "No groups in this report",
                    "Run verification after indexing files. Cancellation and permission warnings may limit the report.",
                );
            }
            for group in &report.groups {
                let name = group
                    .files
                    .first()
                    .map_or_else(|| "Duplicate group".to_string(), |path| file_name(path));
                let extension = std::path::Path::new(&name)
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                let (icon, color) = extension_style(&p, &extension);
                Card::new().hover().padding(16.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        kit::icon_tile(ui, icon, color, 38.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            kit::label(ui, &name, 15.0, Weight::SemiBold, p.text);
                            kit::label(
                                ui,
                                format!(
                                    "{} each · {} logically redundant",
                                    bytes(group.file_size),
                                    bytes(group.reclaimable_size)
                                ),
                                12.5,
                                Weight::Regular,
                                p.text_2,
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            kit::badge_icon(
                                ui,
                                Some(icons::COPY),
                                &format!("{} copies", group.file_count),
                                p.purple,
                            );
                        });
                    });
                    kit::disclosure(ui, "Compare locations", &group.id, |ui| {
                        for path in &group.files {
                            ui.horizontal(|ui| {
                                kit::icon(ui, icons::FILE, 14.0, p.text_3);
                                kit::mono(ui, truncate_middle(&short_path(path), 80), 12.0)
                                    .on_hover_text(path);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if Button::ghost("Reveal")
                                        .icon(icons::ARROW_SQUARE_OUT)
                                        .small()
                                        .show(ui)
                                        .clicked()
                                    {
                                        reveal = Some(path.clone());
                                    }
                                });
                            });
                        }
                        if group.file_count > group.files.len() as u64 {
                            kit::caption(
                                ui,
                                format!(
                                    "{} paths shown of {}",
                                    group.files.len(),
                                    group.file_count
                                ),
                            );
                        }
                        kit::caption(ui, &group.verification);
                    });
                });
            }
            for warning in &report.warnings {
                kit::banner(
                    ui,
                    icons::WARNING,
                    &format!("{}: {}", humanize(&warning.mechanism), warning.detail),
                    p.amber,
                    false,
                );
            }
        } else if self.queries.waiting() {
            kit::skeleton(ui, 6);
        } else {
            kit::empty_state(
                ui,
                icons::FINGERPRINT,
                "Verify before deciding",
                "Content analysis runs locally and never chooses which copy should be removed.",
            );
        }
        if let Some(path) = reveal {
            self.reveal(&path);
        }
        self.pager(ui);
    }
}

fn extension_style(p: &Palette, extension: &str) -> (&'static str, Color32) {
    match extension {
        "zip" | "tar" | "gz" | "7z" | "rar" | "xz" | "bz2" => (icons::FILE_ZIP, p.amber),
        "jpg" | "jpeg" | "png" | "gif" | "heic" | "webp" | "svg" | "tiff" => {
            (icons::FILE_IMAGE, p.cyan)
        }
        "mp4" | "mov" | "mkv" | "avi" | "m4v" => (icons::FILE_VIDEO, p.orange),
        "mp3" | "wav" | "flac" | "aac" | "m4a" => (icons::FILE_AUDIO, p.green),
        "pdf" => (icons::FILE_PDF, p.rose),
        "rs" | "js" | "ts" | "py" | "go" | "swift" | "c" | "h" | "java" | "rb" => {
            (icons::FILE_CODE, p.blue)
        }
        "dmg" | "iso" | "img" => (icons::HARD_DRIVE, p.orange),
        "txt" | "md" | "doc" | "docx" | "rtf" | "pages" => (icons::FILE_TEXT, p.teal),
        _ => (icons::FILE, p.purple),
    }
}
