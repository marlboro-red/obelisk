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
    pub fn preset_default() -> Self {
        Self {
            primary: Color::Rgb(255, 103, 0),
            accent: Color::Rgb(0, 255, 65),
            secondary: Color::Rgb(148, 0, 211),
            danger: Color::Rgb(255, 40, 40),
            info: Color::Rgb(0, 160, 255),
            warn: Color::Rgb(255, 191, 0),
            dark_bg: Color::Rgb(5, 5, 10),
            panel_bg: Color::Rgb(10, 10, 18),
            muted: Color::Rgb(70, 70, 90),
            bright: Color::Rgb(200, 200, 220),
            dim_accent: Color::Rgb(0, 120, 40),
        }
    }

    pub fn preset_solarized() -> Self {
        Self {
            primary: Color::Rgb(181, 137, 0),    // yellow
            accent: Color::Rgb(133, 153, 0),     // green
            secondary: Color::Rgb(108, 113, 196), // violet
            danger: Color::Rgb(220, 50, 47),     // red
            info: Color::Rgb(38, 139, 210),      // blue
            warn: Color::Rgb(203, 75, 22),       // orange
            dark_bg: Color::Rgb(0, 43, 54),      // base03
            panel_bg: Color::Rgb(7, 54, 66),     // base02
            muted: Color::Rgb(88, 110, 117),     // base01
            bright: Color::Rgb(238, 232, 213),   // base2
            dim_accent: Color::Rgb(42, 161, 152), // cyan
        }
    }

    pub fn preset_nord() -> Self {
        Self {
            primary: Color::Rgb(136, 192, 208),   // nord8  frost
            accent: Color::Rgb(163, 190, 140),    // nord14 green
            secondary: Color::Rgb(180, 142, 173), // nord15 purple
            danger: Color::Rgb(191, 97, 106),     // nord11 red
            info: Color::Rgb(129, 161, 193),      // nord9  blue
            warn: Color::Rgb(235, 203, 139),      // nord13 yellow
            dark_bg: Color::Rgb(46, 52, 64),      // nord0
            panel_bg: Color::Rgb(59, 66, 82),     // nord1
            muted: Color::Rgb(76, 86, 106),       // nord3
            bright: Color::Rgb(229, 233, 240),    // nord6
            dim_accent: Color::Rgb(143, 188, 187), // nord7
        }
    }

    pub fn preset_catppuccin() -> Self {
        // Catppuccin Mocha
        Self {
            primary: Color::Rgb(245, 194, 231),   // pink
            accent: Color::Rgb(166, 227, 161),     // green
            secondary: Color::Rgb(203, 166, 247),  // mauve
            danger: Color::Rgb(243, 139, 168),     // red
            info: Color::Rgb(137, 180, 250),       // blue
            warn: Color::Rgb(249, 226, 175),       // yellow
            dark_bg: Color::Rgb(30, 30, 46),       // base
            panel_bg: Color::Rgb(49, 50, 68),      // surface0
            muted: Color::Rgb(88, 91, 112),        // overlay0
            bright: Color::Rgb(205, 214, 244),     // text
            dim_accent: Color::Rgb(148, 226, 213), // teal
        }
    }

    pub fn preset_gruvbox() -> Self {
        Self {
            primary: Color::Rgb(254, 128, 25),     // orange
            accent: Color::Rgb(184, 187, 38),      // green
            secondary: Color::Rgb(211, 134, 155),   // purple
            danger: Color::Rgb(251, 73, 52),       // red
            info: Color::Rgb(131, 165, 152),       // blue
            warn: Color::Rgb(250, 189, 47),        // yellow
            dark_bg: Color::Rgb(40, 40, 40),       // bg
            panel_bg: Color::Rgb(60, 56, 54),      // bg1
            muted: Color::Rgb(146, 131, 116),      // gray
            bright: Color::Rgb(235, 219, 178),     // fg
            dim_accent: Color::Rgb(142, 192, 124), // bright green
        }
    }

    /// Build a theme from the config, using a preset as the base and applying
    /// any individual color overrides.
    pub fn from_config(config: &ThemeConfig) -> Self {
        let mut theme = match config.preset.as_deref() {
            Some("solarized") => Self::preset_solarized(),
            Some("nord") => Self::preset_nord(),
            Some("catppuccin") => Self::preset_catppuccin(),
            Some("gruvbox") => Self::preset_gruvbox(),
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
