//! Embedded typography. Inter carries interface text at four weights, JetBrains Mono carries
//! paths and identifiers, and Phosphor carries icons. Bundling the fonts keeps the hierarchy
//! identical on every platform instead of depending on whatever the OS ships.
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, RichText};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weight {
    Regular,
    Medium,
    SemiBold,
    Bold,
}

const INTER: &str = "inter";
const INTER_MEDIUM: &str = "inter-medium";
const INTER_SEMIBOLD: &str = "inter-semibold";
const INTER_BOLD: &str = "inter-bold";
const MONO: &str = "jetbrains-mono";
const MONO_MEDIUM: &str = "jetbrains-mono-medium";
const ICONS: &str = "phosphor";
const ICONS_FILL: &str = "phosphor-fill";

/// Register every bundled font family. Named families fall back to the proportional stack so
/// punctuation and emoji still resolve when a weight lacks a glyph.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let bundled: [(&str, &'static [u8]); 8] = [
        (INTER, include_bytes!("../assets/fonts/Inter-Regular.ttf")),
        (
            INTER_MEDIUM,
            include_bytes!("../assets/fonts/Inter-Medium.ttf"),
        ),
        (
            INTER_SEMIBOLD,
            include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
        ),
        (INTER_BOLD, include_bytes!("../assets/fonts/Inter-Bold.ttf")),
        (
            MONO,
            include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
        ),
        (
            MONO_MEDIUM,
            include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf"),
        ),
        (ICONS, include_bytes!("../assets/fonts/Phosphor.ttf")),
        (
            ICONS_FILL,
            include_bytes!("../assets/fonts/Phosphor-Fill.ttf"),
        ),
    ];
    for (name, bytes) in bundled {
        let mut data = FontData::from_static(bytes);
        if name == ICONS || name == ICONS_FILL {
            // Icon glyphs sit on the baseline; nudge them so they centre on the text line.
            data.tweak = egui::FontTweak {
                y_offset_factor: 0.06,
                ..Default::default()
            };
        }
        fonts.font_data.insert(name.into(), Arc::new(data));
    }
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, INTER.into());
    let fallbacks = proportional.clone();
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, MONO.into());
    for primary in [
        INTER_MEDIUM,
        INTER_SEMIBOLD,
        INTER_BOLD,
        MONO_MEDIUM,
        ICONS,
        ICONS_FILL,
    ] {
        let mut stack = vec![primary.to_string()];
        stack.extend(fallbacks.iter().cloned());
        fonts
            .families
            .insert(FontFamily::Name(primary.into()), stack);
    }
    ctx.set_fonts(fonts);
}

pub fn family(weight: Weight) -> FontFamily {
    match weight {
        Weight::Regular => FontFamily::Proportional,
        Weight::Medium => FontFamily::Name(INTER_MEDIUM.into()),
        Weight::SemiBold => FontFamily::Name(INTER_SEMIBOLD.into()),
        Weight::Bold => FontFamily::Name(INTER_BOLD.into()),
    }
}

pub fn font(size: f32, weight: Weight) -> FontId {
    FontId::new(size, family(weight))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn mono_medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MONO_MEDIUM.into()))
}

pub fn icon_font(size: f32, filled: bool) -> FontId {
    FontId::new(
        size,
        FontFamily::Name(if filled { ICONS_FILL } else { ICONS }.into()),
    )
}

/// Convenience constructor for styled inline text.
pub fn text(text: impl Into<String>, size: f32, weight: Weight, color: Color32) -> RichText {
    RichText::new(text).font(font(size, weight)).color(color)
}
