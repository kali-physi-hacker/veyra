use super::*;
use eframe::egui::{Align, Layout, Sense, vec2};
use kit::Card;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProcessSort {
    Memory,
    Cpu,
    Io,
}

impl App {
    pub(super) fn system(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let t = self.reveal;
        let Some(snapshot) = self.system.clone() else {
            if self.queries.loading() {
                kit::skeleton(ui, 8);
            } else {
                kit::empty_state(
                    ui,
                    icons::CPU,
                    "No sample yet",
                    "Refresh to take a resource snapshot of this machine.",
                );
            }
            return;
        };
        ui.columns(3, |cols| {
            gauge(
                &mut cols[0],
                "CPU",
                snapshot.cpu_percent / 100.0,
                &format!("{:.0}%", snapshot.cpu_percent),
                &format!(
                    "Sampled over {} ms · load {}",
                    snapshot.sample_millis,
                    snapshot
                        .load_average
                        .iter()
                        .map(|l| format!("{l:.2}"))
                        .collect::<Vec<_>>()
                        .join(" / ")
                ),
                p.green,
                p.teal,
                t,
            );
            gauge(
                &mut cols[1],
                "Memory",
                percent(snapshot.used_memory, snapshot.total_memory),
                &bytes(snapshot.used_memory),
                &format!(
                    "of {}{}",
                    bytes(snapshot.total_memory),
                    snapshot
                        .memory_pressure
                        .as_ref()
                        .map_or(String::new(), |m| format!(" · pressure {m}"))
                ),
                p.accent,
                p.accent_2,
                t,
            );
            gauge(
                &mut cols[2],
                "Swap",
                percent(snapshot.used_swap, snapshot.total_swap),
                &bytes(snapshot.used_swap),
                &format!("of {}", bytes(snapshot.total_swap)),
                p.amber,
                p.orange,
                t,
            );
        });
        ui.add_space(4.0);
        kit::section(
            ui,
            "Volumes",
            "Capacity as reported by the operating system",
            |_| {},
        );
        Card::new().padding(10.0).show(ui, |ui| {
            for volume in &snapshot.volumes {
                let used = volume.total_bytes.saturating_sub(volume.available_bytes);
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    kit::icon_tile(
                        ui,
                        if volume.removable {
                            icons::EJECT
                        } else {
                            icons::HARD_DRIVE
                        },
                        p.blue,
                        34.0,
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.horizontal(|ui| {
                            kit::label(ui, &volume.name, 14.0, Weight::Medium, p.text);
                            kit::caption(ui, format!("{} · {}", volume.mount, volume.filesystem));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                kit::label(
                                    ui,
                                    format!(
                                        "{} free of {}",
                                        bytes(volume.available_bytes),
                                        bytes(volume.total_bytes)
                                    ),
                                    12.5,
                                    Weight::Regular,
                                    p.text_2,
                                );
                            });
                        });
                        let width = ui.available_width() - 6.0;
                        let ratio = percent(used, volume.total_bytes);
                        kit::progress(
                            ui,
                            width,
                            6.0,
                            ratio * t,
                            if ratio > 0.9 { p.rose } else { p.blue },
                        );
                    });
                });
                ui.add_space(6.0);
            }
            if snapshot.volumes.is_empty() {
                kit::caption(ui, "No volumes reported.");
            }
        });
        ui.add_space(4.0);
        let mut sort = self.process_sort;
        kit::section(
            ui,
            "Processes",
            "Snapshot at sample time; refresh to sample again",
            |ui| {
                kit::segmented(
                    ui,
                    egui::Id::new("process-sort"),
                    &mut sort,
                    &[
                        (ProcessSort::Memory, "Memory"),
                        (ProcessSort::Cpu, "CPU"),
                        (ProcessSort::Io, "I/O written"),
                    ],
                );
            },
        );
        self.process_sort = sort;
        let mut processes = snapshot.processes.clone();
        match sort {
            ProcessSort::Memory => processes.sort_by_key(|p| std::cmp::Reverse(p.memory_bytes)),
            ProcessSort::Cpu => processes.sort_by(|a, b| {
                b.cpu_percent
                    .partial_cmp(&a.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            ProcessSort::Io => processes.sort_by_key(|p| std::cmp::Reverse(p.disk_written_bytes)),
        }
        Card::new().padding(8.0).show(ui, |ui| {
            let columns = [70.0, 80.0, 100.0, 110.0];
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                kit::eyebrow(ui, "Process");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(8.0);
                    for (width, label) in
                        columns.iter().zip(["I/O written", "Memory", "CPU", "PID"])
                    {
                        ui.allocate_ui_with_layout(
                            vec2(*width, 20.0),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                kit::eyebrow(ui, label);
                            },
                        );
                    }
                });
            });
            let max_memory = processes
                .iter()
                .map(|p| p.memory_bytes)
                .max()
                .unwrap_or(1)
                .max(1);
            for process in processes.iter().take(100) {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
                let h = ui.ctx().animate_bool_with_time(
                    response.id.with("hover"),
                    response.hovered(),
                    0.12,
                );
                let painter = ui.painter();
                if h > 0.0 {
                    painter.rect_filled(rect, 8.0, p.wash(0.04 * h));
                }
                let bar_width =
                    (rect.width() - 12.0 - columns.iter().sum::<f32>() - 24.0).max(60.0);
                let usage = process.memory_bytes as f32 / max_memory as f32;
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left() + 12.0, rect.bottom() - 4.0),
                        vec2(bar_width * usage * t, 2.0),
                    ),
                    1.0,
                    p.accent.gamma_multiply(0.45),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink2(vec2(12.0, 0.0)))
                        .layout(Layout::left_to_right(Align::Center)),
                );
                kit::label(
                    &mut child,
                    truncate(&process.name, 48),
                    13.5,
                    Weight::Medium,
                    p.text,
                );
                child.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let cells = [
                        bytes(process.disk_written_bytes),
                        bytes(process.memory_bytes),
                        format!("{:.1}%", process.cpu_percent),
                        process.pid.to_string(),
                    ];
                    for (width, value) in columns.iter().zip(cells) {
                        ui.allocate_ui_with_layout(
                            vec2(*width, 20.0),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new(value)
                                        .font(fonts::mono(12.0))
                                        .color(p.text_2),
                                );
                            },
                        );
                    }
                });
            }
        });
        for limitation in &snapshot.limitations {
            kit::caption(ui, limitation);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn gauge(
    ui: &mut egui::Ui,
    title: &str,
    ratio: f32,
    value: &str,
    detail: &str,
    color: Color32,
    color_2: Color32,
    t: f32,
) {
    let p = kit::palette(ui.ctx());
    Card::new().padding(16.0).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            kit::ring(
                ui,
                124.0,
                11.0,
                ratio.clamp(0.0, 1.0) * t,
                color,
                color_2,
                value,
                title,
            );
            ui.add_space(4.0);
            kit::label(ui, detail, 12.0, Weight::Regular, p.text_2);
        });
    });
}
