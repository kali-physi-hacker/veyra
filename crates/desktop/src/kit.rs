//! Stratum's interface kit: cards, gradient buttons, badges, rings, rows, fields, segmented
//! controls and motion helpers, painted directly with epaint so every page shares one visual
//! language. Widgets read the active [`Palette`] from egui memory, so pages never pass colours.
use crate::{
    fonts::{self, Weight},
    icons,
    theme::Palette,
};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Id, InnerResponse, Layout,
    Margin, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, emath,
    epaint, pos2, vec2,
};
use std::{f32::consts::TAU, sync::Arc, time::Instant};

const PALETTE_KEY: &str = "stratum.palette";
const GRADIENT_KEY: &str = "stratum.gradient";

pub fn set_palette(ctx: &egui::Context, palette: Palette) {
    ctx.data_mut(|d| d.insert_temp(Id::new(PALETTE_KEY), palette));
}
pub fn palette(ctx: &egui::Context) -> Palette {
    ctx.data(|d| d.get_temp::<Palette>(Id::new(PALETTE_KEY)))
        .unwrap_or_else(Palette::dark)
}

/// A cached accent gradient texture used to fill primary controls with rounded corners.
fn gradient(ctx: &egui::Context) -> egui::TextureId {
    let key = Id::new(GRADIENT_KEY);
    let p = palette(ctx);
    let signature = (p.accent, p.accent_2);
    if let Some((existing, handle)) =
        ctx.data(|d| d.get_temp::<((Color32, Color32), egui::TextureHandle)>(key))
        && existing == signature
    {
        return handle.id();
    }
    let (w, h) = (96usize, 24usize);
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let t = x as f32 / (w - 1) as f32;
            let highlight = 1.0 - y as f32 / (h - 1) as f32;
            let color = p
                .accent
                .lerp_to_gamma(p.accent_2, t)
                .lerp_to_gamma(Color32::WHITE, highlight * 0.12);
            pixels.push(color);
        }
    }
    let handle = ctx.load_texture(
        GRADIENT_KEY,
        egui::ColorImage::new([w, h], pixels),
        egui::TextureOptions::LINEAR,
    );
    let id = handle.id();
    ctx.data_mut(|d| d.insert_temp(key, (signature, handle)));
    id
}

pub fn gradient_rect(ui: &Ui, rect: Rect, radius: f32, tint: Color32) {
    let texture = gradient(ui.ctx());
    ui.painter().add(
        epaint::RectShape::filled(rect, radius, tint)
            .with_texture(texture, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))),
    );
}

/// A blurred rounded rectangle: soft shadows and glows.
pub fn glow(ui: &Ui, rect: Rect, radius: f32, color: Color32, blur: f32) {
    ui.painter()
        .add(epaint::RectShape::filled(rect, radius, color).with_blur_width(blur));
}

// ---------------------------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------------------------

pub fn label(
    ui: &mut Ui,
    text: impl Into<String>,
    size: f32,
    weight: Weight,
    color: Color32,
) -> Response {
    ui.label(fonts::text(text, size, weight, color))
}
pub fn heading(ui: &mut Ui, text: impl Into<String>) -> Response {
    let p = palette(ui.ctx());
    label(ui, text, 26.0, Weight::Bold, p.text)
}
pub fn muted(ui: &mut Ui, text: impl Into<String>) -> Response {
    let p = palette(ui.ctx());
    label(ui, text, 13.0, Weight::Regular, p.text_2)
}
pub fn caption(ui: &mut Ui, text: impl Into<String>) -> Response {
    let p = palette(ui.ctx());
    label(ui, text, 12.0, Weight::Regular, p.text_3)
}
pub fn eyebrow(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui.ctx());
    label(ui, text.to_uppercase(), 11.0, Weight::SemiBold, p.text_3)
}
pub fn mono(ui: &mut Ui, text: impl Into<String>, size: f32) -> Response {
    let p = palette(ui.ctx());
    ui.label(RichText::new(text).font(fonts::mono(size)).color(p.text_2))
}
/// Monospace text truncated with an ellipsis to the available width instead of wrapping.
pub fn mono_truncated(ui: &mut Ui, text: &str, size: f32) -> Response {
    let p = palette(ui.ctx());
    let galley = galley_truncated(ui, text, fonts::mono(size), p.text_2, ui.available_width());
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, p.text_2);
    response
}
pub fn galley(ui: &Ui, text: &str, font: FontId, color: Color32) -> Arc<epaint::Galley> {
    ui.painter().layout_no_wrap(text.to_owned(), font, color)
}
pub fn galley_truncated(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> Arc<epaint::Galley> {
    let mut job = epaint::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = epaint::text::TextWrapping {
        max_width: max_width.max(8.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// A collapsible section with a Phosphor caret instead of egui's default triangle.
pub fn disclosure(ui: &mut Ui, title: &str, id: impl std::hash::Hash, add: impl FnOnce(&mut Ui)) {
    let p = palette(ui.ctx());
    egui::CollapsingHeader::new(fonts::text(title, 13.0, Weight::Medium, p.text_2))
        .id_salt(id)
        .icon(disclosure_icon)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            add(ui);
        });
}
fn disclosure_icon(ui: &mut Ui, openness: f32, response: &Response) {
    let p = palette(ui.ctx());
    let icon = if openness > 0.5 {
        icons::CARET_DOWN
    } else {
        icons::CARET_RIGHT
    };
    paint_icon(
        ui.painter(),
        response.rect.center(),
        icon,
        12.0,
        p.text_3.lerp_to_gamma(p.text_2, openness),
        false,
    );
}

// ---------------------------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------------------------

pub fn paint_icon(
    painter: &egui::Painter,
    center: Pos2,
    icon: &str,
    size: f32,
    color: Color32,
    filled: bool,
) {
    painter.text(
        center,
        Align2::CENTER_CENTER,
        icon,
        fonts::icon_font(size, filled),
        color,
    );
}
pub fn icon(ui: &mut Ui, icon: &str, size: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size + 2.0), Sense::hover());
    paint_icon(ui.painter(), rect.center(), icon, size, color, false);
    response
}
pub fn paint_icon_tile(painter: &egui::Painter, rect: Rect, icon: &str, color: Color32) {
    let radius = rect.height() * 0.3;
    painter.rect_filled(rect, radius, color);
    let top = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.center().y + 1.0));
    painter.rect_filled(
        top,
        CornerRadius {
            nw: radius as u8,
            ne: radius as u8,
            sw: 0,
            se: 0,
        },
        Color32::from_white_alpha(18),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Color32::from_white_alpha(28)),
        StrokeKind::Inside,
    );
    paint_icon(
        painter,
        rect.center(),
        icon,
        rect.height() * 0.58,
        Color32::WHITE,
        true,
    );
}
/// A coloured rounded square with a white glyph, the signature of every module.
pub fn icon_tile(ui: &mut Ui, icon: &str, color: Color32, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_icon_tile(ui.painter(), rect, icon, color);
    response
}
/// A large soft disc with a coloured glyph, used by empty states and heroes.
pub fn icon_disc(ui: &mut Ui, icon: &str, color: Color32, size: f32) -> Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    painter.circle_filled(rect.center(), size / 2.0, p.soft(color));
    painter.circle_stroke(
        rect.center(),
        size / 2.0,
        Stroke::new(1.0, color.gamma_multiply(0.35)),
    );
    paint_icon(painter, rect.center(), icon, size * 0.5, color, false);
    response
}

// ---------------------------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Primary,
    Secondary,
    Ghost,
    Danger,
    Soft(Color32),
}
pub struct Button {
    label: String,
    icon: Option<&'static str>,
    kind: Kind,
    small: bool,
    enabled: bool,
    min_width: f32,
    trailing: bool,
}
impl Button {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            kind: Kind::Secondary,
            small: false,
            enabled: true,
            min_width: 0.0,
            trailing: false,
        }
    }
    pub fn primary(label: impl Into<String>) -> Self {
        Self::new(label).kind(Kind::Primary)
    }
    pub fn ghost(label: impl Into<String>) -> Self {
        Self::new(label).kind(Kind::Ghost)
    }
    pub fn danger(label: impl Into<String>) -> Self {
        Self::new(label).kind(Kind::Danger)
    }
    pub fn soft(label: impl Into<String>, color: Color32) -> Self {
        Self::new(label).kind(Kind::Soft(color))
    }
    pub fn kind(mut self, kind: Kind) -> Self {
        self.kind = kind;
        self
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn trailing(mut self) -> Self {
        self.trailing = true;
        self
    }
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }
    pub fn show(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (font_size, pad_x, height, icon_size, radius) = if self.small {
            (12.5, 11.0, 28.0, 13.0, 8.0)
        } else {
            (14.0, 16.0, 36.0, 16.0, 10.0)
        };
        let fg = match self.kind {
            Kind::Primary | Kind::Danger => Color32::WHITE,
            Kind::Secondary | Kind::Ghost => p.text,
            Kind::Soft(color) => color,
        };
        let fg = if self.enabled {
            fg
        } else {
            fg.gamma_multiply(0.45)
        };
        let galley = galley(ui, &self.label, fonts::font(font_size, Weight::Medium), fg);
        let icon_span = if self.icon.is_some() {
            icon_size + 7.0
        } else {
            0.0
        };
        let width = (galley.size().x + pad_x * 2.0 + icon_span).max(self.min_width);
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, mut response) = ui.allocate_exact_size(vec2(width, height), sense);
        if self.enabled {
            response = response.on_hover_cursor(CursorIcon::PointingHand);
        }
        let hovered = self.enabled && response.hovered();
        let pressed = self.enabled && response.is_pointer_button_down_on();
        let t = ui
            .ctx()
            .animate_bool_with_time(response.id.with("hover"), hovered, 0.12);
        let alpha = if self.enabled { 1.0 } else { 0.45 };
        match self.kind {
            Kind::Primary => {
                if self.enabled && !pressed {
                    glow(
                        ui,
                        rect.translate(vec2(0.0, 4.0)).shrink(2.0),
                        radius,
                        p.accent.gamma_multiply(0.28 + 0.22 * t),
                        12.0,
                    );
                }
                gradient_rect(ui, rect, radius, Color32::WHITE.gamma_multiply(alpha));
            }
            Kind::Danger => {
                ui.painter()
                    .rect_filled(rect, radius, p.rose.gamma_multiply(alpha));
            }
            Kind::Secondary => {
                let fill = p.raised.lerp_to_gamma(p.text, 0.05 * t);
                ui.painter().rect(
                    rect,
                    radius,
                    fill,
                    Stroke::new(1.0, p.border.lerp_to_gamma(p.border_strong, t)),
                    StrokeKind::Inside,
                );
            }
            Kind::Ghost => {
                if t > 0.0 {
                    ui.painter().rect_filled(rect, radius, p.wash(0.07 * t));
                }
            }
            Kind::Soft(color) => {
                ui.painter().rect_filled(
                    rect,
                    radius,
                    color.gamma_multiply((0.14 + 0.08 * t) * alpha),
                );
            }
        }
        let painter = ui.painter();
        if pressed {
            painter.rect_filled(rect, radius, Color32::from_black_alpha(50));
        } else if t > 0.0 && matches!(self.kind, Kind::Primary | Kind::Danger) {
            painter.rect_filled(rect, radius, Color32::from_white_alpha((26.0 * t) as u8));
        }
        let content = galley.size().x + icon_span;
        let mut x = rect.center().x - content / 2.0;
        if let Some(icon) = self.icon
            && !self.trailing
        {
            paint_icon(
                painter,
                pos2(x + icon_size / 2.0, rect.center().y),
                icon,
                icon_size,
                fg,
                false,
            );
            x += icon_size + 7.0;
        }
        painter.galley(
            pos2(x, rect.center().y - galley.size().y / 2.0),
            galley.clone(),
            fg,
        );
        if let Some(icon) = self.icon
            && self.trailing
        {
            paint_icon(
                painter,
                pos2(x + galley.size().x + 7.0 + icon_size / 2.0, rect.center().y),
                icon,
                icon_size,
                fg,
                false,
            );
        }
        response
    }
}

pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str) -> Response {
    icon_button_ex(ui, icon, tooltip, true, 32.0, None)
}
pub fn icon_button_ex(
    ui: &mut Ui,
    icon: &str,
    tooltip: &str,
    enabled: bool,
    size: f32,
    color: Option<Color32>,
) -> Response {
    let p = palette(ui.ctx());
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(size), sense);
    let hovered = enabled && response.hovered();
    let t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("hover"), hovered, 0.12);
    let painter = ui.painter();
    if t > 0.0 {
        painter.rect_filled(rect, size * 0.28, p.wash(0.08 * t));
    }
    let base = color.unwrap_or(p.text_2);
    let fg = if enabled {
        base.lerp_to_gamma(p.text, t)
    } else {
        base.gamma_multiply(0.4)
    };
    paint_icon(painter, rect.center(), icon, size * 0.5, fg, false);
    if enabled {
        response = response.on_hover_cursor(CursorIcon::PointingHand);
    }
    if !tooltip.is_empty() {
        response = response.on_hover_text(tooltip);
    }
    response
}

// ---------------------------------------------------------------------------------------------
// Badges and small indicators
// ---------------------------------------------------------------------------------------------

pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    badge_icon(ui, None, text, color)
}
pub fn badge_icon(ui: &mut Ui, icon: Option<&str>, text: &str, color: Color32) -> Response {
    let p = palette(ui.ctx());
    let galley = galley(ui, text, fonts::font(11.5, Weight::Medium), color);
    let icon_span = if icon.is_some() { 16.0 } else { 0.0 };
    let size = vec2(galley.size().x + 18.0 + icon_span, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        11.0,
        p.soft(color),
        Stroke::new(1.0, color.gamma_multiply(0.28)),
        StrokeKind::Inside,
    );
    let mut x = rect.left() + 9.0;
    if let Some(icon) = icon {
        paint_icon(
            painter,
            pos2(x + 6.0, rect.center().y),
            icon,
            12.0,
            color,
            false,
        );
        x += 16.0;
    }
    painter.galley(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    response
}
/// A keyboard cap for shortcut hints.
pub fn kbd(ui: &mut Ui, key: &str) {
    let p = palette(ui.ctx());
    let galley = galley(ui, key, fonts::mono(11.0), p.text_2);
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 12.0, 20.0), Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        5.0,
        p.sunken,
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    painter.galley(
        pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text_2,
    );
}
/// A tooltip that shows a label followed by keyboard caps.
pub fn shortcut_tooltip(response: Response, label_text: &str, keys: &[&str]) -> Response {
    let label_text = label_text.to_owned();
    let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
    response.on_hover_ui(move |ui| {
        let p = palette(ui.ctx());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            label(ui, &label_text, 13.0, Weight::Regular, p.text);
            ui.add_space(4.0);
            for key in &keys {
                kbd(ui, key);
            }
        });
    })
}
pub fn divider(ui: &mut Ui) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, p.border));
}

// ---------------------------------------------------------------------------------------------
// Cards and layout
// ---------------------------------------------------------------------------------------------

pub struct Card {
    padding: f32,
    hover: bool,
    fill: Option<Color32>,
    stroke: Option<Color32>,
    radius: f32,
    elevated: bool,
    tint: Option<Color32>,
    fill_width: bool,
}
impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}
impl Card {
    pub fn new() -> Self {
        Self {
            padding: 18.0,
            hover: false,
            fill: None,
            stroke: None,
            radius: 16.0,
            elevated: false,
            tint: None,
            fill_width: true,
        }
    }
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }
    pub fn hover(mut self) -> Self {
        self.hover = true;
        self
    }
    pub fn tinted(mut self, color: Color32) -> Self {
        self.tint = Some(color);
        self
    }
    pub fn fill(mut self, color: Color32) -> Self {
        self.fill = Some(color);
        self
    }
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }
    pub fn elevated(mut self) -> Self {
        self.elevated = true;
        self
    }
    pub fn shrink(mut self) -> Self {
        self.fill_width = false;
        self
    }
    pub fn show<R>(self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        let p = palette(ui.ctx());
        let base_fill = match (self.tint, self.fill) {
            (Some(tint), _) => p.surface.blend(tint.gamma_multiply(0.09)),
            (None, Some(fill)) => fill,
            (None, None) => p.surface,
        };
        let base_stroke = self
            .tint
            .map(|tint| tint.gamma_multiply(0.32))
            .or(self.stroke)
            .unwrap_or(p.border);
        let mut frame = egui::Frame::new()
            .fill(base_fill)
            .stroke(Stroke::new(1.0, base_stroke))
            .corner_radius(self.radius)
            .inner_margin(self.padding);
        if self.elevated || !p.dark {
            frame = frame.shadow(epaint::Shadow {
                offset: [0, if self.elevated { 8 } else { 2 }],
                blur: if self.elevated { 28 } else { 10 },
                spread: 0,
                color: p.shadow,
            });
        }
        let mut prepared = frame.begin(ui);
        if self.fill_width {
            let width = prepared.content_ui.available_width();
            prepared.content_ui.set_width(width);
        }
        let inner = add(&mut prepared.content_ui);
        let response = prepared.allocate_space(ui);
        if self.hover {
            let t = ui.ctx().animate_bool_with_time(
                response.id.with("card-hover"),
                response.hovered(),
                0.15,
            );
            prepared.frame.fill = base_fill.lerp_to_gamma(p.raised, t);
            prepared.frame.stroke = Stroke::new(1.0, base_stroke.lerp_to_gamma(p.border_strong, t));
        }
        prepared.paint(ui);
        InnerResponse { inner, response }
    }
}

/// A section title with an optional subtitle and right-aligned actions.
pub fn section(ui: &mut Ui, title: &str, subtitle: &str, actions: impl FnOnce(&mut Ui)) {
    let p = palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            label(ui, title, 17.0, Weight::SemiBold, p.text);
            if !subtitle.is_empty() {
                label(ui, subtitle, 12.5, Weight::Regular, p.text_2);
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
    });
    ui.add_space(6.0);
}

/// A metric tile: eyebrow, large value, supporting detail and a module icon.
pub fn metric(
    ui: &mut Ui,
    label_text: &str,
    value: &str,
    detail: &str,
    icon: &str,
    color: Color32,
) {
    let p = palette(ui.ctx());
    Card::new().padding(16.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                eyebrow(ui, label_text);
                ui.add_space(3.0);
                label(ui, value, 24.0, Weight::SemiBold, p.text);
                label(ui, detail, 12.0, Weight::Regular, p.text_2);
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                icon_tile(ui, icon, color, 36.0);
            });
        });
    });
}

/// A gradient progress ring with a centred value and caption. `ratio` is 0..=1.
#[allow(clippy::too_many_arguments)]
pub fn ring(
    ui: &mut Ui,
    size: f32,
    thickness: f32,
    ratio: f32,
    color: Color32,
    color_2: Color32,
    value: &str,
    caption_text: &str,
) -> Rect {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let center = rect.center();
    let radius = (size - thickness) / 2.0;
    let painter = ui.painter();
    painter.circle_stroke(center, radius, Stroke::new(thickness, p.wash(0.08)));
    let ratio = ratio.clamp(0.0, 1.0);
    if ratio > 0.003 {
        let steps = ((140.0 * ratio).ceil() as usize).max(2);
        let points: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let angle = -TAU / 4.0 + TAU * ratio * i as f32 / steps as f32;
                center + Vec2::angled(angle) * radius
            })
            .collect();
        let start = points[0];
        let end = *points.last().unwrap_or(&start);
        let stroke = epaint::PathStroke::new_uv(thickness, move |_, pos| {
            let v = pos - center;
            let angle = (v.y.atan2(v.x) + TAU / 4.0).rem_euclid(TAU);
            let t = (angle / (TAU * ratio.max(0.001))).clamp(0.0, 1.0);
            color.lerp_to_gamma(color_2, t)
        });
        painter.add(egui::Shape::line(points, stroke));
        painter.circle_filled(start, thickness / 2.0, color);
        painter.circle_filled(end, thickness / 2.0, color.lerp_to_gamma(color_2, 1.0));
    }
    painter.text(
        center - vec2(0.0, size * 0.045),
        Align2::CENTER_CENTER,
        value,
        fonts::font(size * 0.15, Weight::SemiBold),
        p.text,
    );
    painter.text(
        center + vec2(0.0, size * 0.11),
        Align2::CENTER_CENTER,
        caption_text,
        fonts::font((size * 0.075).max(11.0), Weight::Regular),
        p.text_2,
    );
    rect
}

/// A rounded segmented bar; each segment is a fraction of the full width.
pub fn stacked_bar(ui: &mut Ui, height: f32, segments: &[(f32, Color32)]) -> Rect {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, height / 2.0, p.wash(0.08));
    let gap = 2.0;
    let mut x = rect.left();
    for (fraction, color) in segments {
        let width = rect.width() * fraction.clamp(0.0, 1.0);
        if width >= 1.5 {
            let segment = Rect::from_min_max(
                pos2(x, rect.top()),
                pos2((x + width - gap).min(rect.right()), rect.bottom()),
            );
            painter.rect_filled(segment, height * 0.3, *color);
        }
        x += width;
    }
    rect
}
pub fn progress(ui: &mut Ui, width: f32, height: f32, fraction: f32, color: Color32) -> Rect {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, height / 2.0, p.wash(0.08));
    let fill = fraction.clamp(0.0, 1.0) * rect.width();
    if fill > 0.5 {
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(fill.max(height), height)),
            height / 2.0,
            color,
        );
    }
    rect
}

// ---------------------------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------------------------

pub struct Row {
    title: String,
    subtitle: Option<String>,
    trailing: Option<(String, Color32)>,
    icon: Option<(&'static str, Color32)>,
    swatch: Option<Color32>,
    badge: Option<(String, Color32)>,
    chevron: bool,
    selected: bool,
    height: f32,
    mono_subtitle: bool,
    tile: bool,
    enabled: bool,
}
impl Row {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            trailing: None,
            icon: None,
            swatch: None,
            badge: None,
            chevron: false,
            selected: false,
            height: 52.0,
            mono_subtitle: false,
            tile: true,
            enabled: true,
        }
    }
    /// A small colour dot instead of an icon, for legend-style rows.
    pub fn swatch(mut self, color: Color32) -> Self {
        self.swatch = Some(color);
        self
    }
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
    pub fn mono_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self.mono_subtitle = true;
        self
    }
    pub fn trailing(mut self, text: impl Into<String>, color: Color32) -> Self {
        self.trailing = Some((text.into(), color));
        self
    }
    pub fn icon(mut self, icon: &'static str, color: Color32) -> Self {
        self.icon = Some((icon, color));
        self
    }
    pub fn plain_icon(mut self, icon: &'static str, color: Color32) -> Self {
        self.icon = Some((icon, color));
        self.tile = false;
        self
    }
    pub fn badge(mut self, text: impl Into<String>, color: Color32) -> Self {
        self.badge = Some((text.into(), color));
        self
    }
    pub fn chevron(mut self) -> Self {
        self.chevron = true;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn show(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, mut response) = ui.allocate_exact_size(
            vec2(ui.available_width(), self.height),
            if self.enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        if self.enabled {
            response = response.on_hover_cursor(CursorIcon::PointingHand);
        }
        let t = ui.ctx().animate_bool_with_time(
            response.id.with("row-hover"),
            self.enabled && response.hovered(),
            0.12,
        );
        let painter = ui.painter();
        let radius = 10.0;
        if self.selected {
            painter.rect(
                rect,
                radius,
                p.accent.gamma_multiply(0.14),
                Stroke::new(1.0, p.accent.gamma_multiply(0.45)),
                StrokeKind::Inside,
            );
        } else if t > 0.0 {
            painter.rect_filled(rect, radius, p.wash(0.05 * t));
        }
        let cy = rect.center().y;
        let mut x = rect.left() + 12.0;
        if let Some(color) = self.swatch {
            painter.circle_filled(pos2(x + 5.0, cy), 5.0, color);
            x += 20.0;
        }
        if let Some((icon, color)) = self.icon {
            if self.tile {
                paint_icon_tile(
                    painter,
                    Rect::from_center_size(pos2(x + 15.0, cy), Vec2::splat(30.0)),
                    icon,
                    color,
                );
                x += 42.0;
            } else {
                paint_icon(painter, pos2(x + 9.0, cy), icon, 18.0, color, false);
                x += 30.0;
            }
        }
        let mut right = rect.right() - 12.0;
        if self.chevron {
            paint_icon(
                painter,
                pos2(right - 6.0, cy),
                icons::CARET_RIGHT,
                12.0,
                p.text_3,
                false,
            );
            right -= 22.0;
        }
        if let Some((text, color)) = &self.trailing {
            let galley = galley(ui, text, fonts::font(13.0, Weight::Medium), *color);
            painter.galley(
                pos2(right - galley.size().x, cy - galley.size().y / 2.0),
                galley.clone(),
                *color,
            );
            right -= galley.size().x + 14.0;
        }
        if let Some((text, color)) = &self.badge {
            let galley = galley(ui, text, fonts::font(11.0, Weight::Medium), *color);
            let width = galley.size().x + 16.0;
            let badge_rect =
                Rect::from_center_size(pos2(right - width / 2.0, cy), vec2(width, 20.0));
            painter.rect_filled(badge_rect, 10.0, p.soft(*color));
            painter.galley(
                pos2(badge_rect.left() + 8.0, cy - galley.size().y / 2.0),
                galley,
                *color,
            );
            right -= width + 12.0;
        }
        let available = (right - x).max(24.0);
        let title_color = if self.enabled { p.text } else { p.text_3 };
        let title = galley_truncated(
            ui,
            &self.title,
            fonts::font(
                14.0,
                if self.selected {
                    Weight::SemiBold
                } else {
                    Weight::Medium
                },
            ),
            title_color,
            available,
        );
        if let Some(subtitle) = &self.subtitle {
            let font = if self.mono_subtitle {
                fonts::mono(11.5)
            } else {
                fonts::font(12.0, Weight::Regular)
            };
            let sub = galley_truncated(ui, subtitle, font, p.text_2, available);
            let total = title.size().y + sub.size().y + 2.0;
            let top = cy - total / 2.0;
            painter.galley(pos2(x, top), title.clone(), title_color);
            painter.galley(pos2(x, top + title.size().y + 2.0), sub, p.text_2);
        } else {
            painter.galley(pos2(x, cy - title.size().y / 2.0), title, title_color);
        }
        response
    }
}

// ---------------------------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------------------------

/// A rounded text well with an optional leading icon. Returns the inner text edit response.
pub fn text_field(
    ui: &mut Ui,
    value: &mut String,
    hint: &str,
    icon: Option<&str>,
    width: f32,
    monospace: bool,
) -> Response {
    let p = palette(ui.ctx());
    let height = 36.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let background = ui.painter().add(egui::Shape::Noop);
    let inner = rect.shrink2(vec2(12.0, 0.0));
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 8.0;
    if let Some(icon) = icon {
        self::icon(&mut child, icon, 15.0, p.text_3);
    }
    let font = if monospace {
        fonts::mono(13.0)
    } else {
        fonts::font(14.0, Weight::Regular)
    };
    let response = child.add(
        egui::TextEdit::singleline(value)
            .frame(false)
            .font(font)
            .text_color(p.text)
            .hint_text(RichText::new(hint).color(p.text_3))
            .desired_width(f32::INFINITY)
            .vertical_align(Align::Center)
            .margin(Margin::ZERO),
    );
    let focused = response.has_focus();
    let t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("focus"), focused, 0.15);
    ui.painter().set(
        background,
        epaint::RectShape::new(
            rect,
            10.0,
            p.sunken,
            Stroke::new(1.0, p.border.lerp_to_gamma(p.accent, t)),
            StrokeKind::Inside,
        ),
    );
    if t > 0.0 {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            12.0,
            Stroke::new(2.0, p.accent.gamma_multiply(0.25 * t)),
            StrokeKind::Outside,
        );
    }
    response
}
pub fn search_field(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> Response {
    text_field(ui, value, hint, Some(icons::MAGNIFYING_GLASS), width, false)
}

/// A pill-shaped segmented control with an animated thumb. Returns true when the value changed.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: Id,
    current: &mut T,
    options: &[(T, &str)],
) -> bool {
    let p = palette(ui.ctx());
    let font = fonts::font(13.0, Weight::Medium);
    let height = 32.0;
    let pad = 14.0;
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, label)| galley(ui, label, font.clone(), p.text).size().x + pad * 2.0)
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 6.0;
    let (rect, _) = ui.allocate_exact_size(vec2(total, height), Sense::hover());
    let mut cells = Vec::with_capacity(options.len());
    let mut x = rect.left() + 3.0;
    for width in &widths {
        cells.push(Rect::from_min_size(
            pos2(x, rect.top() + 3.0),
            vec2(*width, height - 6.0),
        ));
        x += width;
    }
    let mut changed = false;
    let mut responses = Vec::with_capacity(options.len());
    for (index, (value, _)) in options.iter().enumerate() {
        let response = ui
            .interact(cells[index], id.with(index), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        if response.clicked() && *current != *value {
            *current = *value;
            changed = true;
        }
        responses.push(response);
    }
    let selected = options
        .iter()
        .position(|(value, _)| value == current)
        .unwrap_or(0);
    let thumb_x =
        ui.ctx()
            .animate_value_with_time(id.with("thumb-x"), cells[selected].left(), 0.18);
    let thumb_w =
        ui.ctx()
            .animate_value_with_time(id.with("thumb-w"), cells[selected].width(), 0.18);
    let painter = ui.painter();
    painter.rect(
        rect,
        height / 2.0,
        p.sunken,
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let thumb = Rect::from_min_size(pos2(thumb_x, rect.top() + 3.0), vec2(thumb_w, height - 6.0));
    painter.rect(
        thumb,
        (height - 6.0) / 2.0,
        if p.dark { p.raised } else { p.surface },
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    for (index, (_, label)) in options.iter().enumerate() {
        let hovered = responses[index].hovered();
        let color = if index == selected {
            p.text
        } else if hovered {
            p.text.lerp_to_gamma(p.text_2, 0.4)
        } else {
            p.text_2
        };
        painter.text(
            cells[index].center(),
            Align2::CENTER_CENTER,
            *label,
            font.clone(),
            color,
        );
    }
    changed
}

/// A rounded checkbox with an animated check mark.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, enabled: bool) -> Response {
    let p = palette(ui.ctx());
    let size = 20.0;
    let (rect, mut response) = ui.allocate_exact_size(
        Vec2::splat(size),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if enabled && response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    if enabled {
        response = response.on_hover_cursor(CursorIcon::PointingHand);
    }
    let t = ui.ctx().animate_bool_with_time_and_easing(
        response.id.with("check"),
        *checked,
        0.16,
        emath::easing::cubic_out,
    );
    let h = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        enabled && response.hovered(),
        0.12,
    );
    let painter = ui.painter();
    let fill = p.sunken.lerp_to_gamma(p.accent, t);
    let stroke = p
        .border_strong
        .lerp_to_gamma(p.accent, (t + h * 0.6).min(1.0));
    let alpha = if enabled { 1.0 } else { 0.5 };
    painter.rect(
        rect,
        6.0,
        fill.gamma_multiply(alpha),
        Stroke::new(1.2, stroke.gamma_multiply(alpha)),
        StrokeKind::Inside,
    );
    if t > 0.02 {
        let a = rect.min + vec2(5.0, 10.5);
        let b = rect.min + vec2(8.5, 14.0);
        let c = rect.min + vec2(15.0, 6.5);
        let mut points = vec![a];
        if t < 0.5 {
            points.push(a.lerp(b, t * 2.0));
        } else {
            points.push(b);
            points.push(b.lerp(c, (t - 0.5) * 2.0));
        }
        painter.add(egui::Shape::line(
            points,
            Stroke::new(2.2, Color32::WHITE.gamma_multiply(alpha)),
        ));
    }
    response
}

/// A numbered step indicator; steps before `active` render as completed.
pub fn steps(ui: &mut Ui, labels: &[&str], active: usize) {
    let p = palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (index, label_text) in labels.iter().enumerate() {
            let done = index < active;
            let current = index == active;
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
            let painter = ui.painter();
            if done || current {
                painter.circle_filled(rect.center(), 12.0, p.accent);
            } else {
                painter.circle_stroke(rect.center(), 11.5, Stroke::new(1.2, p.border_strong));
            }
            if done {
                paint_icon(
                    painter,
                    rect.center(),
                    icons::CHECK,
                    13.0,
                    Color32::WHITE,
                    false,
                );
            } else {
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    (index + 1).to_string(),
                    fonts::font(12.0, Weight::SemiBold),
                    if current { Color32::WHITE } else { p.text_3 },
                );
            }
            label(
                ui,
                *label_text,
                13.0,
                if current {
                    Weight::SemiBold
                } else {
                    Weight::Medium
                },
                if current {
                    p.text
                } else if done {
                    p.text_2
                } else {
                    p.text_3
                },
            );
            if index + 1 < labels.len() {
                let (line, _) = ui.allocate_exact_size(vec2(28.0, 2.0), Sense::hover());
                ui.painter()
                    .rect_filled(line, 1.0, if done { p.accent } else { p.border_strong });
            }
        }
    });
}

/// A calm empty state with an icon disc, title and explanation.
pub fn empty_state(ui: &mut Ui, icon: &str, title_text: &str, detail: &str) {
    let p = palette(ui.ctx());
    Card::new().padding(30.0).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            icon_disc(ui, icon, p.accent, 64.0);
            ui.add_space(10.0);
            label(ui, title_text, 17.0, Weight::SemiBold, p.text);
            ui.add_space(2.0);
            ui.set_max_width(460.0);
            ui.label(fonts::text(detail, 13.0, Weight::Regular, p.text_2));
        });
    });
}

/// An animated arc spinner.
pub fn spinner(ui: &mut Ui, size: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let time = ui.input(|i| i.time) as f32;
    ui.ctx().request_repaint();
    let painter = ui.painter();
    let radius = size / 2.0 - 2.0;
    let start = time * 3.2 % TAU;
    let sweep = TAU * 0.68;
    let steps = 32;
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let angle = start + sweep * i as f32 / steps as f32;
            rect.center() + Vec2::angled(angle) * radius
        })
        .collect();
    let stroke = epaint::PathStroke::new_uv(2.6, move |_, pos| {
        let angle = (pos - rect.center()).angle();
        let t = ((angle - start).rem_euclid(TAU) / sweep).clamp(0.0, 1.0);
        color.gamma_multiply(0.15 + 0.85 * t)
    });
    painter.add(egui::Shape::line(points, stroke));
    response
}

/// Pulsing placeholder lines while a query is in flight.
pub fn skeleton(ui: &mut Ui, rows: usize) {
    let p = palette(ui.ctx());
    let time = ui.input(|i| i.time) as f32;
    ui.ctx().request_repaint();
    let pulse = 0.05 + 0.03 * (time * 2.6).sin();
    for index in 0..rows {
        let fraction = [0.92, 0.64, 0.78, 0.55][index % 4];
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width() * fraction, 14.0), Sense::hover());
        ui.painter().rect_filled(rect, 7.0, p.wash(pulse));
        ui.add_space(6.0);
    }
}

/// A tinted notice with an icon and optional dismiss control. Returns true when dismissed.
pub fn banner(ui: &mut Ui, icon: &str, message: &str, color: Color32, dismissible: bool) -> bool {
    let p = palette(ui.ctx());
    Card::new()
        .tinted(color)
        .padding(12.0)
        .radius(12.0)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                self::icon(ui, icon, 18.0, color);
                let dismiss_width = if dismissible { 40.0 } else { 0.0 };
                let width = (ui.available_width() - dismiss_width).max(80.0);
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    ui.add(
                        egui::Label::new(fonts::text(message, 13.0, Weight::Regular, p.text))
                            .wrap(),
                    );
                });
                if dismissible {
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        icon_button(ui, icons::X, "Dismiss").clicked()
                    })
                    .inner
                } else {
                    false
                }
            })
            .inner
        })
        .inner
}

// ---------------------------------------------------------------------------------------------
// Motion
// ---------------------------------------------------------------------------------------------

/// Eased 0..=1 progress since `started`, requesting repaints until the animation completes.
pub fn entrance(ctx: &egui::Context, started: Instant, duration: f32) -> f32 {
    let t = (started.elapsed().as_secs_f32() / duration.max(0.001)).clamp(0.0, 1.0);
    if t < 1.0 {
        ctx.request_repaint();
    }
    emath::easing::cubic_out(t)
}
/// Apply an entrance fade and slide to the content that follows.
pub fn reveal(ui: &mut Ui, t: f32) {
    if t < 1.0 {
        ui.set_opacity(0.15 + 0.85 * t);
        ui.add_space((1.0 - t) * 14.0);
    }
}

// ---------------------------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------------------------

/// Icon and colour for an indexed category or cleanup candidate category.
pub fn category_style(p: &Palette, category: &str) -> (&'static str, Color32) {
    match category {
        "build_artifacts" | "developer_build_artifact" | "developer_storage" => {
            (icons::CODE, p.purple)
        }
        "code" => (icons::FILE_CODE, p.blue),
        "downloads" => (icons::DOWNLOAD_SIMPLE, p.blue),
        "logs" => (icons::RECEIPT, p.text_3),
        "virtual_machines" => (icons::MONITOR, p.cyan),
        "package_caches" | "package_cache" | "developer_environments" => (icons::PACKAGE, p.purple),
        "images" => (icons::IMAGE, p.cyan),
        "documents" => (icons::FILE_TEXT, p.teal),
        "disk_images" => (icons::HARD_DRIVE, p.orange),
        "containers" => (icons::CUBE, p.cyan),
        "caches" | "application_caches" => (icons::STACK, p.amber),
        "audio" => (icons::MUSIC_NOTE, p.green),
        "video" => (icons::FILM_STRIP, p.orange),
        "archives" => (icons::FILE_ZIP, p.amber),
        "applications" => (icons::APP_WINDOW, p.rose),
        "directory" => (icons::FOLDER_SIMPLE, p.teal),
        _ => (icons::CIRCLE_DASHED, p.text_3),
    }
}
/// Icon and colour for an intelligence finding kind.
pub fn insight_style(p: &Palette, kind: &str, severity: &str) -> (&'static str, Color32) {
    let color = if severity == "warning" {
        p.amber
    } else {
        p.accent
    };
    let icon = if kind.contains("developer") {
        icons::CODE
    } else if kind.contains("growth") {
        icons::TREND_UP
    } else if kind.contains("recent") {
        icons::FILE_PLUS
    } else if kind.contains("application") {
        icons::APP_WINDOW
    } else {
        icons::LIGHTBULB
    };
    (icon, color)
}
/// Icon and colour for an audit record action.
pub fn audit_style(p: &Palette, action: &str) -> (&'static str, Color32) {
    if action.contains("restore") {
        (icons::ARROW_COUNTER_CLOCKWISE, p.teal)
    } else if action.contains("cleanup") {
        (icons::ARCHIVE_BOX, p.amber)
    } else if action.contains("scan") {
        (icons::SCAN, p.blue)
    } else if action.contains("plan") {
        (icons::LIST_CHECKS, p.purple)
    } else if action.contains("config") {
        (icons::GEAR_SIX, p.text_3)
    } else if action.contains("fail") || action.contains("error") {
        (icons::WARNING_CIRCLE, p.rose)
    } else if action.contains("daemon") || action.contains("watch") {
        (icons::PULSE, p.cyan)
    } else {
        (icons::CIRCLE, p.text_3)
    }
}
/// Colour for a scan, plan or quarantine status word.
pub fn status_color(p: &Palette, status: &str) -> Color32 {
    match status {
        "completed" | "restored" | "probably_fresh" | "verified" | "moved" => p.teal,
        "partial" | "stale" | "pending" | "running" => p.amber,
        "failed" | "cancelled" | "conflict" | "error" | "skipped" => p.rose,
        _ => p.text_3,
    }
}
