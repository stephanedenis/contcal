//! Thème : détection du terminal, couleurs des mois, fonds teintés.

use std::env;

use crate::color::Rgb;
use crate::config::{BackgroundChoice, ColorChoice, Settings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    TrueColor,
    Ansi256,
    Mono,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub mode: ColorMode,
    pub dark: bool,
    pub fg: Rgb,
    pub muted: Rgb,
    pub faint: Rgb,
    /// Couleur de fond supposée du terminal (sert uniquement aux mélanges).
    pub base: Rgb,
    pub palette: [Rgb; 12],
    pub today: Rgb,
    pub tint: u8,
    pub weekend_dim: bool,
    pub show_event_time: bool,
}

impl Theme {
    pub fn new(settings: &Settings) -> Self {
        let mode = resolve_mode(settings.color_choice);
        let dark = resolve_dark(settings.background_choice);
        let (base, fg, muted, faint) = if dark {
            (
                Rgb::new(0x11, 0x13, 0x18),
                Rgb::new(0xe7, 0xe9, 0xef),
                Rgb::new(0xa0, 0xa6, 0xb3),
                Rgb::new(0x6b, 0x72, 0x80),
            )
        } else {
            (
                Rgb::new(0xfb, 0xfb, 0xfd),
                Rgb::new(0x24, 0x26, 0x2c),
                Rgb::new(0x5d, 0x63, 0x70),
                Rgb::new(0x8b, 0x91, 0x9d),
            )
        };

        Self {
            mode,
            dark,
            fg,
            muted,
            faint,
            base,
            palette: settings.palette,
            today: settings.today_color,
            tint: settings.month_tint,
            weekend_dim: settings.weekend_dim,
            show_event_time: settings.show_event_time,
        }
    }

    pub fn truecolor(&self) -> bool {
        self.mode == ColorMode::TrueColor
    }

    pub fn accent(&self, month: u32) -> Rgb {
        let idx = month.saturating_sub(1) as usize;
        self.palette.get(idx).copied().unwrap_or(self.palette[0])
    }

    /// Variante lisible de la teinte du mois sur le fond du terminal.
    pub fn accent_fg(&self, month: u32) -> Rgb {
        let accent = self.accent(month);
        if self.dark { accent } else { accent.darken(55) }
    }

    /// Variante lisible de la couleur « aujourd'hui ».
    pub fn today_fg(&self) -> Rgb {
        if self.dark {
            self.today
        } else {
            self.today.darken(45)
        }
    }

    /// Fond d'une cellule : mélange du fond du terminal et de la teinte du mois.
    pub fn month_bg(&self, month: u32) -> Rgb {
        if self.tint == 0 || self.mode == ColorMode::Mono {
            return self.base;
        }
        self.base.blend(self.accent(month), self.tint)
    }

    /// Fond légèrement plus marqué pour le jour courant.
    pub fn today_bg(&self, month: u32) -> Rgb {
        self.base
            .blend(self.accent(month), self.tint.saturating_add(14).min(55))
    }

    /// Couleur du texte d'un jour : teinte du mois, atténuée le week-end.
    pub fn day_fg(&self, month: u32, weekend: bool) -> Rgb {
        let accent = self.accent_fg(month);
        if weekend && self.weekend_dim {
            let amount = if self.dark { 30 } else { 22 };
            accent.blend(self.base, amount)
        } else {
            accent
        }
    }

    pub fn event_fg(&self) -> Rgb {
        self.fg
    }

    pub fn event_time_fg(&self) -> Rgb {
        self.muted
    }

    pub fn source_text_fg(&self, source: Rgb) -> Rgb {
        if self.dark {
            source.lighten(8)
        } else {
            source.darken(22)
        }
    }

    pub fn border(&self) -> Rgb {
        self.faint
    }

    pub fn error(&self) -> Rgb {
        if self.dark {
            Rgb::new(0xf7, 0x76, 0x8e)
        } else {
            Rgb::new(0xc0, 0x39, 0x4b)
        }
    }

    pub fn warning(&self) -> Rgb {
        if self.dark {
            Rgb::new(0xe0, 0xaf, 0x68)
        } else {
            Rgb::new(0x9a, 0x67, 0x00)
        }
    }

    pub fn success(&self) -> Rgb {
        if self.dark {
            Rgb::new(0x9e, 0xce, 0x6a)
        } else {
            Rgb::new(0x3d, 0x7f, 0x2f)
        }
    }
}

fn resolve_mode(choice: ColorChoice) -> ColorMode {
    match choice {
        ColorChoice::TrueColor => ColorMode::TrueColor,
        ColorChoice::Ansi256 => ColorMode::Ansi256,
        ColorChoice::Mono => ColorMode::Mono,
        ColorChoice::Auto => {
            if no_color() || term_is_dumb() {
                ColorMode::Mono
            } else if supports_truecolor() {
                ColorMode::TrueColor
            } else {
                ColorMode::Ansi256
            }
        }
    }
}

fn resolve_dark(choice: BackgroundChoice) -> bool {
    match choice {
        BackgroundChoice::Dark => true,
        BackgroundChoice::Light => false,
        BackgroundChoice::Auto => detect_dark_background(),
    }
}

fn no_color() -> bool {
    env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

fn term_is_dumb() -> bool {
    env::var("TERM")
        .map(|t| t.eq_ignore_ascii_case("dumb"))
        .unwrap_or(false)
}

fn supports_truecolor() -> bool {
    if let Ok(ct) = env::var("COLORTERM") {
        let ct = ct.to_ascii_lowercase();
        if ct.contains("truecolor") || ct.contains("24bit") {
            return true;
        }
    }
    if env::var_os("WT_SESSION").is_some() {
        return true;
    }
    let term_program = env::var("TERM_PROGRAM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        term_program.as_str(),
        "wezterm" | "iterm.app" | "vscode" | "ghostty" | "kitty" | "alacritty" | "apple_terminal"
    ) {
        // Apple Terminal ne gère pas le 24 bits, mais COLORTERM est souvent absent.
        return term_program != "apple_terminal";
    }
    if let Ok(term) = env::var("TERM") {
        let term = term.to_ascii_lowercase();
        if term.contains("truecolor")
            || term.contains("kitty")
            || term.contains("alacritty")
            || term.contains("wezterm")
            || term.contains("ghostty")
        {
            return true;
        }
    }
    false
}

fn detect_dark_background() -> bool {
    if let Ok(fgbg) = env::var("COLORFGBG") {
        // Format classique : "fg;bg" (parfois "fg;default;bg").
        let bg = fgbg.rsplit(';').next().unwrap_or("").trim();
        if let Ok(idx) = bg.parse::<u8>() {
            // 0-6 et 8 sont sombres ; 7 et 9-15 sont clairs.
            return !(idx == 7 || (9..=15).contains(&idx));
        }
    }
    if let Ok(scheme) = env::var("TERM_BACKGROUND")
        && scheme.eq_ignore_ascii_case("light")
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    fn settings() -> Settings {
        let cfg: config::Config = toml::from_str(config::DEFAULT_CONFIG).unwrap();
        cfg.to_settings(std::path::PathBuf::from("/tmp/x"))
    }

    #[test]
    fn month_colors_cycle_and_tint() {
        let mut s = settings();
        s.color_choice = ColorChoice::TrueColor;
        s.background_choice = BackgroundChoice::Dark;
        let theme = Theme::new(&s);
        assert_eq!(theme.accent(1), theme.palette[0]);
        assert_eq!(theme.accent(12), theme.palette[11]);
        assert_eq!(theme.accent(99), theme.palette[0]);
        let bg1 = theme.month_bg(1);
        let bg7 = theme.month_bg(7);
        assert_ne!(bg1, bg7);
        // La teinte reste proche du fond : elle ne doit pas devenir la couleur pure.
        assert!(bg1.contrast(theme.accent(1)) < 12.0);
    }

    #[test]
    fn mono_has_no_tint() {
        let mut s = settings();
        s.background_choice = BackgroundChoice::Dark;
        s.color_choice = ColorChoice::Mono;
        let theme = Theme::new(&s);
        assert_eq!(theme.mode, ColorMode::Mono);
        assert_eq!(theme.month_bg(3), theme.base);
    }

    #[test]
    fn weekend_is_dimmer() {
        let mut s = settings();
        s.background_choice = BackgroundChoice::Dark;
        let theme = Theme::new(&s);
        let weekday = theme.day_fg(7, false);
        let weekend = theme.day_fg(7, true);
        assert_ne!(weekday, weekend);
        assert!(weekend.contrast(theme.base) < weekday.contrast(theme.base));
    }
}
