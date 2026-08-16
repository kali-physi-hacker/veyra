//! The Stratum mark: a gradient tile with three strata, painted live for the sidebar and
//! rasterised once for the window and dock icon.
use crate::theme::Palette;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};

/// Paint the logo tile into `rect`.
pub fn paint_mark(painter: &egui::Painter, rect: Rect, p: &Palette) {
    let radius = rect.height() * 0.28;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), p.accent);
    mesh.colored_vertex(rect.right_top(), p.accent.lerp_to_gamma(p.accent_2, 0.55));
    mesh.colored_vertex(rect.right_bottom(), p.accent_2);
    mesh.colored_vertex(rect.left_bottom(), p.accent.lerp_to_gamma(p.accent_2, 0.45));
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    // A rounded mask is not available for meshes, so paint the rounded tile in the base colour
    // first and let the gradient show through a slightly inset mesh.
    painter.rect_filled(rect, radius, p.accent.lerp_to_gamma(p.accent_2, 0.5));
    let inset = rect.shrink(rect.height() * 0.09);
    let mut inner = egui::Mesh::default();
    inner.colored_vertex(inset.left_top(), p.accent);
    inner.colored_vertex(inset.right_top(), p.accent.lerp_to_gamma(p.accent_2, 0.55));
    inner.colored_vertex(inset.right_bottom(), p.accent_2);
    inner.colored_vertex(
        inset.left_bottom(),
        p.accent.lerp_to_gamma(p.accent_2, 0.45),
    );
    inner.add_triangle(0, 1, 2);
    inner.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(inner));
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Color32::from_white_alpha(40)),
        StrokeKind::Inside,
    );
    let bar_height = rect.height() * 0.11;
    let left = rect.left() + rect.width() * 0.24;
    for (i, width) in [0.52f32, 0.40, 0.30].iter().enumerate() {
        let y = rect.top() + rect.height() * (0.30 + 0.20 * i as f32);
        let bar = Rect::from_min_size(pos2(left, y), vec2(rect.width() * width, bar_height));
        painter.rect_filled(
            bar,
            bar_height / 2.0,
            Color32::from_white_alpha(if i == 0 { 240 } else { 190 - 40 * i as u8 }),
        );
    }
}

/// Rasterise the mark for the window/dock icon.
pub fn icon_data(size: u32) -> egui::IconData {
    let p = Palette::dark();
    let s = size as f32;
    let radius = s * 0.22;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let bars = [
        (0.24f32, 0.30f32, 0.52f32, 1.0f32),
        (0.24, 0.50, 0.40, 0.78),
        (0.24, 0.70, 0.30, 0.62),
    ];
    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let coverage = rounded_coverage(px, py, s, radius);
            if coverage <= 0.0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let t = ((px + py) / (2.0 * s)).clamp(0.0, 1.0);
            let mut color = p.accent.lerp_to_gamma(p.accent_2, t);
            let highlight = 1.0 - py / s;
            color = color.lerp_to_gamma(Color32::WHITE, highlight * 0.10);
            let mut white = 0.0f32;
            for (bx, by, bw, alpha) in bars {
                let bar = Rect::from_min_size(pos2(bx * s, by * s), vec2(bw * s, s * 0.11));
                let c = bar_coverage(pos2(px, py), bar);
                white = white.max(c * alpha);
            }
            let r = lerp(color.r() as f32, 255.0, white);
            let g = lerp(color.g() as f32, 255.0, white);
            let b = lerp(color.b() as f32, 255.0, white);
            rgba.extend_from_slice(&[r as u8, g as u8, b as u8, (coverage * 255.0) as u8]);
        }
    }
    egui::IconData {
        rgba,
        width: size,
        height: size,
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Anti-aliased coverage of a rounded square of side `s` at point (x, y).
fn rounded_coverage(x: f32, y: f32, s: f32, radius: f32) -> f32 {
    let half = s / 2.0;
    let dx = (x - half).abs() - (half - radius);
    let dy = (y - half).abs() - (half - radius);
    let outside = vec2(dx.max(0.0), dy.max(0.0)).length();
    let inside = dx.max(dy).min(0.0);
    let distance = outside + inside - radius;
    (0.5 - distance).clamp(0.0, 1.0)
}

fn bar_coverage(point: Pos2, bar: Rect) -> f32 {
    let radius = bar.height() / 2.0;
    let center = bar.center();
    let dx = (point.x - center.x).abs() - (bar.width() / 2.0 - radius);
    let dy = (point.y - center.y).abs() - (bar.height() / 2.0 - radius);
    let outside = vec2(dx.max(0.0), dy.max(0.0)).length();
    let inside = dx.max(dy).min(0.0);
    let distance = outside + inside - radius;
    (0.5 - distance).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icon_is_opaque_inside_and_transparent_at_corners() {
        let icon = icon_data(64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
        let alpha = |x: usize, y: usize| icon.rgba[(y * 64 + x) * 4 + 3];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(32, 32), 255);
        assert_eq!(alpha(63, 0), 0);
    }
}
