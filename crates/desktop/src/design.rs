use super::*;
pub const ACCENT: Color32 = Color32::from_rgb(166, 159, 255);
pub const AMBER: Color32 = Color32::from_rgb(241, 190, 116);
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .corner_radius(14)
        .inner_margin(18)
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(43, 48, 65)))
}
pub fn eyebrow(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(10.5)
            .color(MUTED)
            .strong(),
    );
}
pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.14))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(color));
        });
}
pub fn age(timestamp: i64) -> String {
    let elapsed = now().saturating_sub(timestamp).max(0);
    if elapsed < 60 {
        "just now".into()
    } else if elapsed < 3600 {
        format!("{}m ago", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{}h ago", elapsed / 3600)
    } else {
        format!("{}d ago", elapsed / 86400)
    }
}
pub fn short_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME")
        && let Ok(rest) = std::path::Path::new(path).strip_prefix(&home)
    {
        return format!("~/{}", rest.display())
            .trim_end_matches('/')
            .to_string();
    }
    path.into()
}
pub fn ring(ui: &mut egui::Ui, used: f32, available: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(168.0), egui::Sense::hover());
    let center = rect.center();
    ui.painter().circle_stroke(
        center,
        68.0,
        egui::Stroke::new(10.0, Color32::from_rgb(47, 50, 69)),
    );
    let ratio = used.clamp(0.0, 1.0);
    if ratio > 0.0 {
        let points = (0..=80)
            .map(|i| {
                let angle =
                    -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * ratio * i as f32 / 80.0;
                center + Vec2::angled(angle) * 68.0
            })
            .collect();
        ui.painter()
            .add(egui::Shape::line(points, egui::Stroke::new(10.0, ACCENT)));
    }
    ui.painter().text(
        center - Vec2::new(0.0, 8.0),
        egui::Align2::CENTER_CENTER,
        available,
        egui::FontId::proportional(24.0),
        Color32::WHITE,
    );
    ui.painter().text(
        center + Vec2::new(0.0, 20.0),
        egui::Align2::CENTER_CENTER,
        "available",
        egui::FontId::proportional(12.0),
        MUTED,
    );
}
pub fn empty(ui: &mut egui::Ui, title: &str, detail: &str) {
    card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.heading(title);
        ui.label(RichText::new(detail).color(MUTED));
    });
}
