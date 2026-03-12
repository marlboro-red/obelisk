use ratatui::style::Color;
use serde::{Deserialize, Serialize};

/// All configurable colors used by the UI.
#[derive(Debug, Clone)]
pub struct Theme {
    pub primary: Color,
    pub accent: Color,
    pub secondary: Color,
    pub danger: Color,
    pub info: Color,
    pub warn: Color,
    pub dark_bg: Color,
    pub panel_bg: Color,
    pub muted: Color,
    pub bright: Color,
    pub dim_accent: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::preset_default()
    }
}

impl Theme {
    /// Default: "Obsidian" — steel-blue accent on a near-black base.
    /// Monochromatic foundation with 2-3 restrained accent colors.
    pub fn preset_default() -> Self {
        Self {
            primary: Color::Rgb(110, 160, 210),   // steel blue — borders, focus, titles
            accent: Color::Rgb(110, 175, 140),     // sage — success, completion
            secondary: Color::Rgb(130, 130, 150),  // blue-gray — subdued IDs, metadata
            danger: Color::Rgb(190, 80, 75),       // muted rust — errors, failures
            info: Color::Rgb(110, 160, 210),       // = primary (minimal palette)
            warn: Color::Rgb(195, 165, 90),        // muted gold — caution, cost
            dark_bg: Color::Rgb(14, 14, 20),       // near-black
            panel_bg: Color::Rgb(20, 20, 28),      // panel background
            muted: Color::Rgb(60, 60, 75),         // dim gray — labels, chrome
            bright: Color::Rgb(185, 185, 200),     // silver — primary text
            dim_accent: Color::Rgb(60, 95, 75),    // dimmed sage
        }
    }

    /// "Frost" — cool teal accent on a dark cool-gray base.
    pub fn preset_solarized() -> Self {
        Self {
            primary: Color::Rgb(100, 170, 190),    // teal
            accent: Color::Rgb(120, 180, 160),      // sea glass
            secondary: Color::Rgb(120, 130, 140),   // cool gray
            danger: Color::Rgb(180, 85, 90),        // cool red
            info: Color::Rgb(100, 170, 190),        // = primary
            warn: Color::Rgb(180, 165, 110),        // muted gold
            dark_bg: Color::Rgb(12, 14, 18),        // cool near-black
            panel_bg: Color::Rgb(18, 20, 26),       // cool panel
            muted: Color::Rgb(55, 62, 75),          // cool dark gray
            bright: Color::Rgb(180, 190, 200),      // cool silver
            dim_accent: Color::Rgb(65, 95, 85),     // dimmed teal
        }
    }

    /// "Ember" — warm amber accent on a dark warm-gray base.
    pub fn preset_nord() -> Self {
        Self {
            primary: Color::Rgb(200, 160, 110),     // warm amber
            accent: Color::Rgb(160, 175, 120),       // olive
            secondary: Color::Rgb(140, 130, 125),    // warm gray
            danger: Color::Rgb(190, 85, 70),         // warm red
            info: Color::Rgb(200, 160, 110),         // = primary
            warn: Color::Rgb(200, 170, 100),         // amber
            dark_bg: Color::Rgb(18, 16, 14),         // warm near-black
            panel_bg: Color::Rgb(26, 24, 22),        // warm panel
            muted: Color::Rgb(75, 70, 65),           // warm dark gray
            bright: Color::Rgb(195, 190, 180),       // warm silver
            dim_accent: Color::Rgb(90, 100, 70),     // dim olive
        }
    }

    /// "Ash" — near-pure grayscale with a faint blue highlight.
    pub fn preset_catppuccin() -> Self {
        Self {
            primary: Color::Rgb(140, 160, 185),      // subtle blue-gray
            accent: Color::Rgb(140, 175, 150),        // subtle sage
            secondary: Color::Rgb(130, 130, 135),     // neutral gray
            danger: Color::Rgb(185, 85, 80),          // muted red
            info: Color::Rgb(140, 160, 185),          // = primary
            warn: Color::Rgb(185, 165, 110),          // amber-gray
            dark_bg: Color::Rgb(16, 16, 18),          // near-black
            panel_bg: Color::Rgb(24, 24, 28),         // gray panel
            muted: Color::Rgb(62, 62, 68),            // neutral gray
            bright: Color::Rgb(192, 192, 198),         // silver
            dim_accent: Color::Rgb(75, 95, 80),        // dim sage
        }
    }

    /// "Deep" — deep ocean blue accent on a midnight-blue base.
    pub fn preset_gruvbox() -> Self {
        Self {
            primary: Color::Rgb(90, 140, 200),        // deep blue
            accent: Color::Rgb(100, 165, 145),         // teal-green
            secondary: Color::Rgb(110, 115, 135),      // indigo gray
            danger: Color::Rgb(185, 75, 80),           // red
            info: Color::Rgb(90, 140, 200),            // = primary
            warn: Color::Rgb(180, 155, 95),            // muted gold
            dark_bg: Color::Rgb(10, 10, 18),           // deep midnight
            panel_bg: Color::Rgb(16, 16, 28),          // midnight panel
            muted: Color::Rgb(50, 52, 70),             // midnight gray
            bright: Color::Rgb(175, 180, 200),         // cool silver
            dim_accent: Color::Rgb(55, 90, 80),        // dark teal
        }
    }

    /// Build a theme from the config, using a preset as the base and applying
    /// any individual color overrides.
    pub fn from_config(config: &ThemeConfig) -> Self {
        let mut theme = match config.preset.as_deref() {
            Some("solarized") | Some("frost") => Self::preset_solarized(),
            Some("nord") | Some("ember") => Self::preset_nord(),
            Some("catppuccin") | Some("ash") => Self::preset_catppuccin(),
            Some("gruvbox") | Some("deep") => Self::preset_gruvbox(),
            _ => Self::preset_default(),
        };

        // Apply individual overrides
        if let Some(c) = config.primary.as_deref().and_then(parse_hex_color) {
            theme.primary = c;
        }
        if let Some(c) = config.accent.as_deref().and_then(parse_hex_color) {
            theme.accent = c;
        }
        if let Some(c) = config.secondary.as_deref().and_then(parse_hex_color) {
            theme.secondary = c;
        }
        if let Some(c) = config.danger.as_deref().and_then(parse_hex_color) {
            theme.danger = c;
        }
        if let Some(c) = config.info.as_deref().and_then(parse_hex_color) {
            theme.info = c;
        }
        if let Some(c) = config.warn.as_deref().and_then(parse_hex_color) {
            theme.warn = c;
        }
        if let Some(c) = config.dark_bg.as_deref().and_then(parse_hex_color) {
            theme.dark_bg = c;
        }
        if let Some(c) = config.panel_bg.as_deref().and_then(parse_hex_color) {
            theme.panel_bg = c;
        }
        if let Some(c) = config.muted.as_deref().and_then(parse_hex_color) {
            theme.muted = c;
        }
        if let Some(c) = config.bright.as_deref().and_then(parse_hex_color) {
            theme.bright = c;
        }
        if let Some(c) = config.dim_accent.as_deref().and_then(parse_hex_color) {
            theme.dim_accent = c;
        }

        theme
    }
}

/// TOML-serializable theme configuration.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ThemeConfig {
    pub preset: Option<String>,
    pub primary: Option<String>,
    pub accent: Option<String>,
    pub secondary: Option<String>,
    pub danger: Option<String>,
    pub info: Option<String>,
    pub warn: Option<String>,
    pub dark_bg: Option<String>,
    pub panel_bg: Option<String>,
    pub muted: Option<String>,
    pub bright: Option<String>,
    pub dim_accent: Option<String>,
}

/// Parse a hex color string like "#FF6700" or "FF6700" into a ratatui Color.
fn parse_hex_color(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}
