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

    /// "Dusk" — purple-violet accent on a deep plum-black base.
    pub fn preset_dracula() -> Self {
        Self {
            primary: Color::Rgb(150, 120, 210),     // soft violet
            accent: Color::Rgb(130, 190, 160),       // mint
            secondary: Color::Rgb(125, 120, 145),    // lavender gray
            danger: Color::Rgb(195, 80, 90),         // rose red
            info: Color::Rgb(150, 120, 210),         // = primary
            warn: Color::Rgb(195, 175, 105),         // muted gold
            dark_bg: Color::Rgb(16, 12, 22),         // plum black
            panel_bg: Color::Rgb(24, 18, 32),        // dark plum
            muted: Color::Rgb(60, 55, 75),           // dusky gray
            bright: Color::Rgb(190, 185, 205),       // lavender silver
            dim_accent: Color::Rgb(70, 100, 85),     // dim mint
        }
    }

    /// "Amber" — warm orange-gold accent on a charcoal base.
    pub fn preset_monokai() -> Self {
        Self {
            primary: Color::Rgb(210, 155, 80),       // warm orange
            accent: Color::Rgb(140, 190, 120),        // lime green
            secondary: Color::Rgb(140, 135, 130),     // warm gray
            danger: Color::Rgb(195, 75, 75),          // bright red
            info: Color::Rgb(210, 155, 80),           // = primary
            warn: Color::Rgb(210, 175, 90),           // gold
            dark_bg: Color::Rgb(18, 18, 16),          // charcoal
            panel_bg: Color::Rgb(28, 28, 24),         // dark charcoal
            muted: Color::Rgb(72, 70, 65),            // charcoal gray
            bright: Color::Rgb(198, 195, 185),        // warm white
            dim_accent: Color::Rgb(80, 105, 68),      // dim lime
        }
    }

    /// "Twilight" — cool blue-purple accent on a slate-blue base.
    pub fn preset_tokyo_night() -> Self {
        Self {
            primary: Color::Rgb(120, 145, 220),      // periwinkle
            accent: Color::Rgb(115, 185, 165),        // seafoam
            secondary: Color::Rgb(115, 120, 150),     // slate gray
            danger: Color::Rgb(185, 80, 85),          // muted rose
            info: Color::Rgb(120, 145, 220),          // = primary
            warn: Color::Rgb(190, 170, 100),          // muted amber
            dark_bg: Color::Rgb(12, 14, 24),          // slate black
            panel_bg: Color::Rgb(18, 20, 34),         // dark slate
            muted: Color::Rgb(52, 56, 78),            // slate gray
            bright: Color::Rgb(180, 185, 210),        // cool lilac
            dim_accent: Color::Rgb(62, 100, 90),      // dim seafoam
        }
    }

    /// "Carbon" — muted blue accent on a warm carbon-black base.
    pub fn preset_one_dark() -> Self {
        Self {
            primary: Color::Rgb(105, 155, 210),      // soft blue
            accent: Color::Rgb(125, 180, 140),        // soft green
            secondary: Color::Rgb(130, 130, 140),     // neutral gray
            danger: Color::Rgb(190, 80, 78),          // warm red
            info: Color::Rgb(105, 155, 210),          // = primary
            warn: Color::Rgb(195, 170, 95),           // amber
            dark_bg: Color::Rgb(16, 18, 22),          // carbon black
            panel_bg: Color::Rgb(24, 26, 32),         // dark carbon
            muted: Color::Rgb(60, 64, 72),            // carbon gray
            bright: Color::Rgb(188, 190, 198),        // silver
            dim_accent: Color::Rgb(68, 98, 78),       // dim green
        }
    }

    /// "Bloom" — dusky pink accent on a deep plum base.
    pub fn preset_rose_pine() -> Self {
        Self {
            primary: Color::Rgb(180, 130, 175),      // dusky rose
            accent: Color::Rgb(140, 175, 155),        // sage
            secondary: Color::Rgb(130, 125, 140),     // mauve gray
            danger: Color::Rgb(190, 80, 85),          // muted red
            info: Color::Rgb(180, 130, 175),          // = primary
            warn: Color::Rgb(195, 170, 110),          // muted gold
            dark_bg: Color::Rgb(18, 14, 20),          // deep plum
            panel_bg: Color::Rgb(26, 22, 30),         // plum panel
            muted: Color::Rgb(68, 60, 72),            // plum gray
            bright: Color::Rgb(195, 188, 200),        // rose white
            dim_accent: Color::Rgb(78, 95, 84),       // dim sage
        }
    }

    /// "Moss" — earthy green accent on a dark forest base.
    pub fn preset_everforest() -> Self {
        Self {
            primary: Color::Rgb(130, 175, 130),      // forest green
            accent: Color::Rgb(160, 180, 120),        // olive-lime
            secondary: Color::Rgb(130, 135, 125),     // earthy gray
            danger: Color::Rgb(185, 85, 75),          // earthy red
            info: Color::Rgb(130, 175, 130),          // = primary
            warn: Color::Rgb(190, 170, 100),          // warm amber
            dark_bg: Color::Rgb(14, 18, 14),          // forest black
            panel_bg: Color::Rgb(22, 26, 22),         // dark forest
            muted: Color::Rgb(58, 65, 58),            // forest gray
            bright: Color::Rgb(188, 192, 185),        // sage white
            dim_accent: Color::Rgb(88, 100, 68),      // dim olive
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
            Some("dracula") | Some("dusk") => Self::preset_dracula(),
            Some("monokai") | Some("amber") => Self::preset_monokai(),
            Some("tokyo-night") | Some("twilight") => Self::preset_tokyo_night(),
            Some("one-dark") | Some("carbon") => Self::preset_one_dark(),
            Some("rose-pine") | Some("bloom") => Self::preset_rose_pine(),
            Some("everforest") | Some("moss") => Self::preset_everforest(),
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
#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
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
pub(crate) fn parse_hex_color(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_color_with_hash() {
        assert_eq!(parse_hex_color("#FF6700"), Some(Color::Rgb(255, 103, 0)));
    }

    #[test]
    fn parse_hex_color_without_hash() {
        assert_eq!(parse_hex_color("00FF00"), Some(Color::Rgb(0, 255, 0)));
    }

    #[test]
    fn parse_hex_color_black() {
        assert_eq!(parse_hex_color("#000000"), Some(Color::Rgb(0, 0, 0)));
    }

    #[test]
    fn parse_hex_color_white() {
        assert_eq!(parse_hex_color("#FFFFFF"), Some(Color::Rgb(255, 255, 255)));
    }

    #[test]
    fn parse_hex_color_lowercase() {
        assert_eq!(parse_hex_color("#ff6700"), Some(Color::Rgb(255, 103, 0)));
    }

    #[test]
    fn parse_hex_color_too_short() {
        assert_eq!(parse_hex_color("#FFF"), None);
    }

    #[test]
    fn parse_hex_color_too_long() {
        assert_eq!(parse_hex_color("#FF670000"), None);
    }

    #[test]
    fn parse_hex_color_empty() {
        assert_eq!(parse_hex_color(""), None);
    }

    #[test]
    fn parse_hex_color_invalid_chars() {
        assert_eq!(parse_hex_color("#ZZZZZZ"), None);
    }

    #[test]
    fn parse_hex_color_hash_only() {
        assert_eq!(parse_hex_color("#"), None);
    }

    #[test]
    fn all_presets_produce_rgb_colors() {
        let presets = [
            Theme::preset_default(),
            Theme::preset_solarized(),
            Theme::preset_nord(),
            Theme::preset_catppuccin(),
            Theme::preset_gruvbox(),
            Theme::preset_dracula(),
            Theme::preset_monokai(),
            Theme::preset_tokyo_night(),
            Theme::preset_one_dark(),
            Theme::preset_rose_pine(),
            Theme::preset_everforest(),
        ];
        for theme in &presets {
            assert!(matches!(theme.primary, Color::Rgb(_, _, _)));
            assert!(matches!(theme.accent, Color::Rgb(_, _, _)));
            assert!(matches!(theme.secondary, Color::Rgb(_, _, _)));
            assert!(matches!(theme.danger, Color::Rgb(_, _, _)));
            assert!(matches!(theme.info, Color::Rgb(_, _, _)));
            assert!(matches!(theme.warn, Color::Rgb(_, _, _)));
            assert!(matches!(theme.dark_bg, Color::Rgb(_, _, _)));
            assert!(matches!(theme.panel_bg, Color::Rgb(_, _, _)));
            assert!(matches!(theme.muted, Color::Rgb(_, _, _)));
            assert!(matches!(theme.bright, Color::Rgb(_, _, _)));
            assert!(matches!(theme.dim_accent, Color::Rgb(_, _, _)));
        }
    }

    #[test]
    fn default_theme_matches_preset_default() {
        let def = Theme::default();
        let preset = Theme::preset_default();
        // Compare a representative color — they should match
        assert!(matches!(
            (&def.primary, &preset.primary),
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2))
                if r1 == r2 && g1 == g2 && b1 == b2
        ));
    }

    #[test]
    fn from_config_with_no_preset_uses_default() {
        let config = ThemeConfig::default();
        let theme = Theme::from_config(&config);
        let default = Theme::preset_default();
        assert!(matches!(
            (&theme.primary, &default.primary),
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2))
                if r1 == r2 && g1 == g2 && b1 == b2
        ));
    }

    #[test]
    fn from_config_solarized_preset() {
        let config = ThemeConfig {
            preset: Some("solarized".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_solarized();
        assert!(matches!(
            (&theme.primary, &expected.primary),
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2))
                if r1 == r2 && g1 == g2 && b1 == b2
        ));
    }

    #[test]
    fn from_config_frost_alias_maps_to_solarized() {
        let config = ThemeConfig {
            preset: Some("frost".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_solarized();
        assert!(matches!(
            (&theme.primary, &expected.primary),
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2))
                if r1 == r2 && g1 == g2 && b1 == b2
        ));
    }

    #[test]
    fn from_config_ember_alias_maps_to_nord() {
        let config = ThemeConfig {
            preset: Some("ember".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_nord();
        assert!(matches!(
            (&theme.primary, &expected.primary),
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2))
                if r1 == r2 && g1 == g2 && b1 == b2
        ));
    }

    #[test]
    fn from_config_ash_alias_maps_to_catppuccin() {
        let config = ThemeConfig {
            preset: Some("ash".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_catppuccin();
        assert!(matches!(
            (&theme.primary, &expected.primary),
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2))
                if r1 == r2 && g1 == g2 && b1 == b2
        ));
    }

    #[test]
    fn from_config_deep_alias_maps_to_gruvbox() {
        let config = ThemeConfig {
            preset: Some("deep".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_gruvbox();
        assert!(matches!(
            (&theme.primary, &expected.primary),
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2))
                if r1 == r2 && g1 == g2 && b1 == b2
        ));
    }

    #[test]
    fn from_config_dracula_preset() {
        let config = ThemeConfig {
            preset: Some("dracula".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_dracula();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_dusk_alias_maps_to_dracula() {
        let config = ThemeConfig {
            preset: Some("dusk".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_dracula();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_monokai_preset() {
        let config = ThemeConfig {
            preset: Some("monokai".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_monokai();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_amber_alias_maps_to_monokai() {
        let config = ThemeConfig {
            preset: Some("amber".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_monokai();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_tokyo_night_preset() {
        let config = ThemeConfig {
            preset: Some("tokyo-night".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_tokyo_night();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_twilight_alias_maps_to_tokyo_night() {
        let config = ThemeConfig {
            preset: Some("twilight".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_tokyo_night();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_one_dark_preset() {
        let config = ThemeConfig {
            preset: Some("one-dark".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_one_dark();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_carbon_alias_maps_to_one_dark() {
        let config = ThemeConfig {
            preset: Some("carbon".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_one_dark();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_rose_pine_preset() {
        let config = ThemeConfig {
            preset: Some("rose-pine".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_rose_pine();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_bloom_alias_maps_to_rose_pine() {
        let config = ThemeConfig {
            preset: Some("bloom".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_rose_pine();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_everforest_preset() {
        let config = ThemeConfig {
            preset: Some("everforest".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_everforest();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_moss_alias_maps_to_everforest() {
        let config = ThemeConfig {
            preset: Some("moss".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let expected = Theme::preset_everforest();
        assert_eq!(theme.primary, expected.primary);
    }

    #[test]
    fn from_config_unknown_preset_falls_back_to_default() {
        let config = ThemeConfig {
            preset: Some("nonexistent".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let default = Theme::preset_default();
        assert!(matches!(
            (&theme.primary, &default.primary),
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2))
                if r1 == r2 && g1 == g2 && b1 == b2
        ));
    }

    #[test]
    fn from_config_overrides_individual_colors() {
        let config = ThemeConfig {
            primary: Some("#FF0000".into()),
            accent: Some("00FF00".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        assert_eq!(theme.primary, Color::Rgb(255, 0, 0));
        assert_eq!(theme.accent, Color::Rgb(0, 255, 0));
    }

    #[test]
    fn from_config_invalid_override_keeps_preset_color() {
        let config = ThemeConfig {
            primary: Some("not-a-color".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        let default = Theme::preset_default();
        assert_eq!(theme.primary, default.primary);
    }

    #[test]
    fn from_config_all_overridable_fields() {
        let config = ThemeConfig {
            primary: Some("#010101".into()),
            accent: Some("#020202".into()),
            secondary: Some("#030303".into()),
            danger: Some("#040404".into()),
            info: Some("#050505".into()),
            warn: Some("#060606".into()),
            dark_bg: Some("#070707".into()),
            panel_bg: Some("#080808".into()),
            muted: Some("#090909".into()),
            bright: Some("#0A0A0A".into()),
            dim_accent: Some("#0B0B0B".into()),
            ..Default::default()
        };
        let theme = Theme::from_config(&config);
        assert_eq!(theme.primary, Color::Rgb(1, 1, 1));
        assert_eq!(theme.accent, Color::Rgb(2, 2, 2));
        assert_eq!(theme.secondary, Color::Rgb(3, 3, 3));
        assert_eq!(theme.danger, Color::Rgb(4, 4, 4));
        assert_eq!(theme.info, Color::Rgb(5, 5, 5));
        assert_eq!(theme.warn, Color::Rgb(6, 6, 6));
        assert_eq!(theme.dark_bg, Color::Rgb(7, 7, 7));
        assert_eq!(theme.panel_bg, Color::Rgb(8, 8, 8));
        assert_eq!(theme.muted, Color::Rgb(9, 9, 9));
        assert_eq!(theme.bright, Color::Rgb(10, 10, 10));
        assert_eq!(theme.dim_accent, Color::Rgb(11, 11, 11));
    }

    #[test]
    fn theme_config_serde_round_trip() {
        let config = ThemeConfig {
            preset: Some("nord".into()),
            primary: Some("#FF0000".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ThemeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.preset.as_deref(), Some("nord"));
        assert_eq!(deserialized.primary.as_deref(), Some("#FF0000"));
        assert!(deserialized.accent.is_none());
    }

    #[test]
    fn presets_have_distinct_primary_colors() {
        let presets = [
            Theme::preset_default(),
            Theme::preset_solarized(),
            Theme::preset_nord(),
            Theme::preset_catppuccin(),
            Theme::preset_gruvbox(),
            Theme::preset_dracula(),
            Theme::preset_monokai(),
            Theme::preset_tokyo_night(),
            Theme::preset_one_dark(),
            Theme::preset_rose_pine(),
            Theme::preset_everforest(),
        ];
        // Every pair of presets should have different primary colors
        for i in 0..presets.len() {
            for j in (i + 1)..presets.len() {
                assert_ne!(presets[i].primary, presets[j].primary,
                    "presets {} and {} have identical primary colors", i, j);
            }
        }
    }
}
