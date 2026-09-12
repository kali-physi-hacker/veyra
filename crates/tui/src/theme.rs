//! Palette and text styles shared by every page.
use ratatui::style::{Color, Modifier, Style};

/// Colors used by the terminal UI. The truecolor palette matches the desktop
/// application; the ANSI palette only uses the sixteen standard colors so the
/// interface adapts to terminals without 24-bit color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub bg: Color,
    pub raised: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub teal: Color,
    pub amber: Color,
    pub rose: Color,
    pub blue: Color,
    pub green: Color,
    pub lavender: Color,
    pub cyan: Color,
}
impl Theme {
    pub const fn truecolor() -> Self {
        Self {
            bg: Color::Rgb(0x0E, 0x11, 0x19),
            raised: Color::Rgb(0x1C, 0x21, 0x30),
            border: Color::Rgb(0x2A, 0x30, 0x42),
            text: Color::Rgb(0xE6, 0xEA, 0xF2),
            muted: Color::Rgb(0x8B, 0x9A, 0xB1),
            faint: Color::Rgb(0x5B, 0x65, 0x77),
            accent: Color::Rgb(0xA6, 0x9F, 0xFF),
            teal: Color::Rgb(0x48, 0xD6, 0xB8),
            amber: Color::Rgb(0xF1, 0xBE, 0x74),
            rose: Color::Rgb(0xDF, 0x80, 0x90),
            blue: Color::Rgb(0x68, 0x97, 0xE8),
            green: Color::Rgb(0x7D, 0xD3, 0xA4),
            lavender: Color::Rgb(0xAB, 0x8E, 0xEB),
            cyan: Color::Rgb(0x5C, 0xB0, 0xC7),
        }
    }
    pub const fn ansi() -> Self {
        Self {
            bg: Color::Reset,
            raised: Color::DarkGray,
            border: Color::DarkGray,
            text: Color::Reset,
            muted: Color::Gray,
            faint: Color::DarkGray,
            accent: Color::Magenta,
            teal: Color::Cyan,
            amber: Color::Yellow,
            rose: Color::Red,
            blue: Color::Blue,
            green: Color::Green,
            lavender: Color::LightMagenta,
            cyan: Color::LightCyan,
        }
    }
    pub fn base(&self) -> Style {
        Style::new().fg(self.text).bg(self.bg)
    }
    pub fn text(&self) -> Style {
        Style::new().fg(self.text)
    }
    pub fn strong(&self) -> Style {
        Style::new().fg(self.text).add_modifier(Modifier::BOLD)
    }
    pub fn muted(&self) -> Style {
        Style::new().fg(self.muted)
    }
    pub fn faint(&self) -> Style {
        Style::new().fg(self.faint)
    }
    pub fn accent(&self) -> Style {
        Style::new().fg(self.accent)
    }
    pub fn color(&self, color: Color) -> Style {
        Style::new().fg(color)
    }
    pub fn bold(&self, color: Color) -> Style {
        Style::new().fg(color).add_modifier(Modifier::BOLD)
    }
    pub fn border(&self) -> Style {
        Style::new().fg(self.border)
    }
    /// Row highlight for the focused item of a list or table.
    pub fn selected(&self) -> Style {
        Style::new()
            .fg(self.text)
            .bg(self.raised)
            .add_modifier(Modifier::BOLD)
    }
    /// Section label: small caps feel through upper case and muted color.
    pub fn eyebrow(&self) -> Style {
        Style::new().fg(self.muted).add_modifier(Modifier::BOLD)
    }
    pub const fn categories(&self) -> [Color; 6] {
        [
            self.teal,
            self.blue,
            self.lavender,
            self.amber,
            self.rose,
            self.cyan,
        ]
    }
}
