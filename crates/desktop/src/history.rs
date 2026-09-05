use super::*;
use eframe::egui::{Align, Align2, Layout, Pos2, Rect, Sense, Stroke, pos2, vec2};
use kit::{Card, Row};

impl App {
    pub(super) fn history(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        self.navigation(ui);
        let mut points: Vec<HistoryPoint> = self
            .history
            .iter()
            .filter(|pt| self.path.is_empty() || pt.path == self.path)
            .cloned()
            .collect();
        if self.path.is_empty() {
            kit::empty_state(
                ui,
                icons::CHART_LINE_UP,
                "Choose a directory",
                "Pick an indexed location above to graph comparable observations.",
            );
        } else if points.len() >= 2 {
            points.sort_by_key(|pt| pt.timestamp);
            let peak = points.iter().map(|pt| pt.logical_bytes).max().unwrap_or(0);
            let first = points.first().map(|pt| pt.logical_bytes).unwrap_or(0);
            let last = points.last().map(|pt| pt.logical_bytes).unwrap_or(0);
            let t = self.reveal;
            Card::new().padding(18.0).show(ui, |ui| {
                let path = self.path.clone();
                ui.horizontal(|ui| {
                    kit::icon_tile(ui, icons::CHART_LINE_UP, p.cyan, 38.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (icon, color, label) = if last > first {
                            (
                                icons::TREND_UP,
                                p.amber,
                                format!("+{} since first", bytes(last - first)),
                            )
                        } else if last < first {
                            (
                                icons::TREND_DOWN,
                                p.teal,
                                format!("−{} since first", bytes(first - last)),
                            )
                        } else {
                            (icons::MINUS, p.text_3, "No change since first".to_string())
                        };
                        kit::badge_icon(ui, Some(icon), &label, color);
                        kit::badge_icon(
                            ui,
                            Some(icons::CHART_BAR),
                            &format!("peak {}", bytes(peak)),
                            p.cyan,
                        );
                        kit::badge(ui, &format!("{} observations", points.len()), p.text_3);
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                kit::eyebrow(ui, "Observed logical size");
                                let width = ui.available_width();
                                let galley = kit::galley_truncated(
                                    ui,
                                    &short_path(&path),
                                    fonts::font(15.0, Weight::SemiBold),
                                    p.text,
                                    width,
                                );
                                let (rect, response) =
                                    ui.allocate_exact_size(galley.size(), egui::Sense::hover());
                                ui.painter().galley(rect.min, galley, p.text);
                                response.on_hover_text(&path);
                            });
                        });
                    });
                });
                ui.add_space(8.0);
                chart(ui, &points, p.cyan, t);
            });
        } else {
            kit::empty_state(
                ui,
                icons::HOURGLASS,
                "Not enough observations yet",
                "At least two scans of this directory are needed to show change. Rescan later to add a point.",
            );
        }
        if !self.history.is_empty() {
            kit::section(
                ui,
                "Observations",
                "Directory totals recorded by completed scans",
                |_| {},
            );
            Card::new().padding(8.0).show(ui, |ui| {
                for pt in self.history.iter().rev().take(100) {
                    Row::new(bytes(pt.logical_bytes))
                        .plain_icon(icons::CLOCK, p.cyan)
                        .mono_subtitle(format!(
                            "{} · {} · {}",
                            datetime(pt.timestamp),
                            humanize(&pt.coverage),
                            truncate_middle(&short_path(&pt.path), 60)
                        ))
                        .trailing(age(pt.timestamp), p.text_3)
                        .height(46.0)
                        .enabled(false)
                        .show(ui)
                        .on_hover_text(&pt.path);
                }
            });
        }
    }
}

/// A gradient area chart of logical bytes over time with hover readouts.
fn chart(ui: &mut egui::Ui, points: &[HistoryPoint], color: Color32, t: f32) {
    let p = kit::palette(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 250.0), Sense::hover());
    let plot = Rect::from_min_max(
        pos2(rect.left() + 64.0, rect.top() + 12.0),
        pos2(rect.right() - 12.0, rect.bottom() - 28.0),
    );
    let max = points
        .iter()
        .map(|pt| pt.logical_bytes)
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let first = points[0].timestamp;
    let span = (points.last().map(|pt| pt.timestamp).unwrap_or(first) - first).max(1) as f32;
    let painter = ui.painter();
    for step in 0..=4 {
        let y = plot.bottom() - plot.height() * step as f32 / 4.0;
        painter.hline(
            plot.x_range(),
            y,
            Stroke::new(
                1.0,
                p.border.gamma_multiply(if step == 0 { 1.0 } else { 0.6 }),
            ),
        );
        painter.text(
            pos2(plot.left() - 10.0, y),
            Align2::RIGHT_CENTER,
            bytes((max * step as f32 / 4.0) as u64),
            fonts::font(11.0, Weight::Regular),
            p.text_3,
        );
    }
    let positions: Vec<Pos2> = points
        .iter()
        .map(|pt| {
            pos2(
                plot.left() + (pt.timestamp - first) as f32 / span * plot.width(),
                plot.bottom() - pt.logical_bytes as f32 / max * plot.height() * t,
            )
        })
        .collect();
    let mut mesh = egui::Mesh::default();
    for pair in positions.windows(2) {
        let base = mesh.vertices.len() as u32;
        let top = color.gamma_multiply(0.30);
        let bottom = color.gamma_multiply(0.0);
        mesh.colored_vertex(pair[0], top);
        mesh.colored_vertex(pair[1], top);
        mesh.colored_vertex(pos2(pair[1].x, plot.bottom()), bottom);
        mesh.colored_vertex(pos2(pair[0].x, plot.bottom()), bottom);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.add(egui::Shape::line(
        positions.clone(),
        Stroke::new(2.5, color),
    ));
    for position in &positions {
        painter.circle_filled(*position, 4.0, p.surface);
        painter.circle_stroke(*position, 4.0, Stroke::new(2.0, color));
    }
    painter.text(
        pos2(plot.left(), rect.bottom() - 8.0),
        Align2::LEFT_CENTER,
        date(first),
        fonts::font(11.0, Weight::Regular),
        p.text_3,
    );
    painter.text(
        pos2(plot.right(), rect.bottom() - 8.0),
        Align2::RIGHT_CENTER,
        date(points.last().map(|pt| pt.timestamp).unwrap_or(first)),
        fonts::font(11.0, Weight::Regular),
        p.text_3,
    );
    if let Some(pointer) = response.hover_pos()
        && plot.contains(pointer)
        && let Some((index, position)) = positions.iter().enumerate().min_by(|a, b| {
            (a.1.x - pointer.x)
                .abs()
                .partial_cmp(&(b.1.x - pointer.x).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        let painter = ui.painter();
        painter.vline(
            position.x,
            plot.y_range(),
            Stroke::new(1.0, color.gamma_multiply(0.5)),
        );
        painter.circle_filled(*position, 6.0, color);
        let point = &points[index];
        response.on_hover_ui_at_pointer(|ui| {
            kit::label(
                ui,
                bytes(point.logical_bytes),
                14.0,
                Weight::SemiBold,
                p.text,
            );
            kit::caption(
                ui,
                format!(
                    "{} · {}",
                    datetime(point.timestamp),
                    humanize(&point.coverage)
                ),
            );
        });
    }
}
