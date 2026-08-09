//! Colour tokens and egui style bridging. Every custom widget reads a [`Palette`]; the same
//! palette also restyles egui's built-in widgets so text fields, combo boxes and scroll bars
//! belong to the same family.
use crate::fonts::{self, Weight};
use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke, TextStyle, Vec2, epaint};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub sidebar: Color32,
    pub surface: Color32,
    pub raised: Color32,
    pub sunken: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_2: Color32,
    pub text_3: Color32,
    pub accent: Color32,
    pub accent_2: Color32,
    pub teal: Color32,
    pub amber: Color32,
    pub rose: Color32,
    pub blue: Color32,
    pub green: Color32,
    pub purple: Color32,
    pub cyan: Color32,
    pub orange: Color32,
    pub shadow: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Palette {
    pub const fn dark() -> Self {
        Self {
            dark: true,
            bg: rgb(15, 17, 23),
            sidebar: rgb(11, 13, 19),
            surface: rgb(23, 26, 35),
            raised: rgb(31, 35, 48),
            sunken: rgb(12, 14, 20),
            border: rgb(38, 43, 58),
            border_strong: rgb(62, 69, 90),
            text: rgb(238, 241, 247),
            text_2: rgb(160, 168, 186),
            text_3: rgb(107, 115, 135),
            accent: rgb(139, 124, 255),
            accent_2: rgb(94, 168, 255),
            teal: rgb(61, 214, 180),
            amber: rgb(245, 190, 91),
            rose: rgb(240, 122, 140),
            blue: rgb(91, 155, 255),
            green: rgb(95, 212, 138),
            purple: rgb(181, 140, 255),
            cyan: rgb(79, 200, 232),
            orange: rgb(255, 159, 97),
            shadow: Color32::from_rgba_premultiplied(0, 0, 0, 96),
        }
    }
    pub const fn light() -> Self {
        Self {
            dark: false,
            bg: rgb(244, 245, 249),
            sidebar: rgb(236, 238, 245),
            surface: rgb(255, 255, 255),
            raised: rgb(255, 255, 255),
            sunken: rgb(238, 240, 246),
            border: rgb(226, 229, 238),
            border_strong: rgb(200, 205, 220),
            text: rgb(20, 23, 31),
            text_2: rgb(91, 99, 117),
            text_3: rgb(139, 147, 166),
            accent: rgb(108, 92, 231),
            accent_2: rgb(59, 130, 246),
            teal: rgb(15, 168, 138),
            amber: rgb(217, 154, 30),
            rose: rgb(224, 80, 106),
            blue: rgb(47, 123, 238),
            green: rgb(45, 168, 94),
            purple: rgb(142, 92, 240),
            cyan: rgb(30, 167, 200),
            orange: rgb(236, 120, 60),
            shadow: Color32::from_rgba_premultiplied(15, 20, 40, 26),
        }
    }
    pub fn for_mode(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }
    /// Categorical series colour for charts and map tiles.
    pub fn series(&self, index: usize) -> Color32 {
        [
            self.accent,
            self.teal,
            self.blue,
            self.amber,
            self.rose,
            self.purple,
            self.cyan,
            self.green,
            self.orange,
        ][index % 9]
    }
    /// A translucent tint of `color` suitable as a background behind text in that colour.
    pub fn soft(&self, color: Color32) -> Color32 {
        color.gamma_multiply(if self.dark { 0.16 } else { 0.12 })
    }
    /// The colour of subtle hover washes on top of surfaces.
    pub fn wash(&self, strength: f32) -> Color32 {
        self.text.gamma_multiply(strength)
    }
    /// Restyle egui's own widgets so they blend with the custom kit.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.dark_mode = self.dark;
        visuals.override_text_color = Some(self.text);
        visuals.weak_text_color = Some(self.text_3);
        visuals.panel_fill = self.bg;
        visuals.window_fill = self.surface;
        visuals.extreme_bg_color = self.sunken;
        visuals.faint_bg_color = self.raised;
        visuals.code_bg_color = self.sunken;
        visuals.text_edit_bg_color = Some(self.sunken);
        visuals.hyperlink_color = self.accent;
        visuals.warn_fg_color = self.amber;
        visuals.error_fg_color = self.rose;
        visuals.selection.bg_fill = self.accent.gamma_multiply(0.35);
        visuals.selection.stroke = Stroke::new(1.0, self.accent);
        visuals.text_cursor.stroke.color = self.accent;
        visuals.window_corner_radius = CornerRadius::same(16);
        visuals.menu_corner_radius = CornerRadius::same(12);
        visuals.window_stroke = Stroke::new(1.0, self.border);
        visuals.window_shadow = epaint::Shadow {
            offset: [0, 12],
            blur: 36,
            spread: 0,
            color: self.shadow,
        };
        visuals.popup_shadow = epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: self.shadow,
        };
        let radius = CornerRadius::same(9);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = self.surface;
        widgets.noninteractive.weak_bg_fill = self.surface;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text_2);
        widgets.noninteractive.corner_radius = radius;
        widgets.inactive.bg_fill = self.raised;
        widgets.inactive.weak_bg_fill = self.raised;
        widgets.inactive.bg_stroke = Stroke::new(1.0, self.border);
        widgets.inactive.fg_stroke = Stroke::new(1.0, self.text);
        widgets.inactive.corner_radius = radius;
        widgets.inactive.expansion = 0.0;
        widgets.hovered.bg_fill = self.raised.lerp_to_gamma(self.text, 0.06);
        widgets.hovered.weak_bg_fill = widgets.hovered.bg_fill;
        widgets.hovered.bg_stroke = Stroke::new(1.0, self.border_strong);
        widgets.hovered.fg_stroke = Stroke::new(1.0, self.text);
        widgets.hovered.corner_radius = radius;
        widgets.hovered.expansion = 0.0;
        widgets.active.bg_fill = self.accent.gamma_multiply(0.28);
        widgets.active.weak_bg_fill = widgets.active.bg_fill;
        widgets.active.bg_stroke = Stroke::new(1.0, self.accent);
        widgets.active.fg_stroke = Stroke::new(1.0, self.text);
        widgets.active.corner_radius = radius;
        widgets.active.expansion = 0.0;
        widgets.open.bg_fill = self.raised;
        widgets.open.weak_bg_fill = self.raised;
        widgets.open.bg_stroke = Stroke::new(1.0, self.border_strong);
        widgets.open.fg_stroke = Stroke::new(1.0, self.text);
        widgets.open.corner_radius = radius;

        let mut style = (*ctx.style()).clone();
        style.visuals = visuals;
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(14.0, 7.0);
        style.spacing.interact_size = Vec2::new(40.0, 30.0);
        style.spacing.window_margin = Margin::same(22);
        style.spacing.menu_margin = Margin::same(8);
        style.spacing.indent = 18.0;
        style.spacing.combo_width = 140.0;
        style.spacing.tooltip_width = 360.0;
        style.spacing.scroll = egui::style::ScrollStyle::thin();
        style.animation_time = 0.14;
        style.interaction.selectable_labels = false;
        style.interaction.tooltip_delay = 0.35;
        style
            .text_styles
            .insert(TextStyle::Heading, fonts::font(22.0, Weight::SemiBold));
        style
            .text_styles
            .insert(TextStyle::Body, fonts::font(14.0, Weight::Regular));
        style
            .text_styles
            .insert(TextStyle::Button, fonts::font(14.0, Weight::Medium));
        style
            .text_styles
            .insert(TextStyle::Small, fonts::font(12.0, Weight::Regular));
        style
            .text_styles
            .insert(TextStyle::Monospace, fonts::mono(13.0));
        ctx.all_styles_mut(|s| *s = style.clone());
        ctx.set_theme(if self.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(c: Color32) -> f32 {
        let channel = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }
    fn contrast(a: Color32, b: Color32) -> f32 {
        let (l1, l2) = (luminance(a), luminance(b));
        (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05)
    }
    #[test]
    fn body_text_meets_contrast_on_every_surface() {
        for palette in [Palette::dark(), Palette::light()] {
            for surface in [palette.bg, palette.surface, palette.raised, palette.sidebar] {
                assert!(contrast(palette.text, surface) >= 7.0, "{palette:?}");
                assert!(contrast(palette.text_2, surface) >= 4.5, "{palette:?}");
            }
        }
    }
    #[test]
    fn series_colours_cycle_without_panicking() {
        let palette = Palette::dark();
        assert_eq!(palette.series(0), palette.series(9));
        assert_ne!(palette.series(0), palette.series(1));
    }
}
