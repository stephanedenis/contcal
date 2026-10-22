//! Couleurs RGB et conversions ANSI (24 bits ou palette 256).

use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const BLACK: Self = Self::new(0, 0, 0);
    pub const WHITE: Self = Self::new(255, 255, 255);

    /// Accepte `#rrggbb`, `rrggbb`, `#rgb` et `rgb`.
    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        match s.len() {
            3 => {
                let mut it = s.chars();
                let r = it.next()?.to_digit(16)? as u8;
                let g = it.next()?.to_digit(16)? as u8;
                let b = it.next()?.to_digit(16)? as u8;
                Some(Self::new(r * 17, g * 17, b * 17))
            }
            6 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                Some(Self::new(r, g, b))
            }
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Mélange linéaire : `pct` pour cent de `other`, le reste de `self`.
    pub fn blend(self, other: Rgb, pct: u8) -> Self {
        let p = u32::from(pct.min(100));
        let q = 100 - p;
        let mix = |a: u8, b: u8| -> u8 {
            ((u32::from(a) * q + u32::from(b) * p + 50) / 100).min(255) as u8
        };
        Self::new(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
        )
    }

    pub fn lighten(self, pct: u8) -> Self {
        self.blend(Self::WHITE, pct)
    }

    pub fn darken(self, pct: u8) -> Self {
        self.blend(Self::BLACK, pct)
    }

    /// Luminance relative WCAG (0.0 = noir, 1.0 = blanc).
    pub fn relative_luma(self) -> f32 {
        fn channel(c: u8) -> f32 {
            let c = f32::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// Contraste WCAG entre deux couleurs.
    pub fn contrast(self, other: Rgb) -> f32 {
        let a = self.relative_luma();
        let b = other.relative_luma();
        let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Couleur de texte lisible (noir ou blanc) sur ce fond.
    pub fn readable_text(self) -> Self {
        if self.contrast(Self::BLACK) >= self.contrast(Self::WHITE) {
            Self::new(16, 16, 20)
        } else {
            Self::WHITE
        }
    }

    /// Indice xterm 256 le plus proche (cube 6×6×6 + niveaux de gris).
    pub fn to_ansi256(self) -> u8 {
        // Gris neutres : souvent plus justes que le cube.
        let avg = (u16::from(self.r) + u16::from(self.g) + u16::from(self.b)) / 3;
        let spread = self
            .r
            .max(self.g)
            .max(self.b)
            .saturating_sub(self.r.min(self.g).min(self.b));
        if spread < 12 {
            if avg < 8 {
                return 16;
            }
            if avg > 238 {
                return 231;
            }
            return 232 + ((avg - 8) * 23 / 230) as u8;
        }

        fn q(c: u8) -> u8 {
            if c < 48 {
                0
            } else if c < 114 {
                1
            } else {
                ((u16::from(c) - 35) / 40).min(5) as u8
            }
        }
        let r = q(self.r);
        let g = q(self.g);
        let b = q(self.b);
        16 + 36 * r + 6 * g + b
    }

    /// Séquence ANSI de couleur de premier plan.
    pub fn fg_code(self, truecolor: bool) -> String {
        let mut s = String::new();
        if truecolor {
            let _ = write!(s, "\x1b[38;2;{};{};{}m", self.r, self.g, self.b);
        } else {
            let _ = write!(s, "\x1b[38;5;{}m", self.to_ansi256());
        }
        s
    }

    /// Séquence ANSI de couleur de fond.
    pub fn bg_code(self, truecolor: bool) -> String {
        let mut s = String::new();
        if truecolor {
            let _ = write!(s, "\x1b[48;2;{};{};{}m", self.r, self.g, self.b);
        } else {
            let _ = write!(s, "\x1b[48;5;{}m", self.to_ansi256());
        }
        s
    }
}

/// Palette de base : une teinte par mois (janvier → décembre).
pub const DEFAULT_PALETTE: [Rgb; 12] = [
    Rgb::new(0x6e, 0xa8, 0xfe), // janvier  — bleu
    Rgb::new(0xb1, 0x97, 0xfc), // février  — violet
    Rgb::new(0x4d, 0xd4, 0xc6), // mars     — turquoise
    Rgb::new(0x74, 0xc6, 0x9d), // avril    — vert
    Rgb::new(0xb5, 0xe4, 0x8c), // mai      — lime
    Rgb::new(0xf4, 0xb8, 0x60), // juin     — ambre
    Rgb::new(0xff, 0x9f, 0x68), // juillet  — orange
    Rgb::new(0xef, 0x6f, 0x6c), // août     — corail
    Rgb::new(0xd0, 0xa2, 0xff), // septembre— mauve
    Rgb::new(0xff, 0xc8, 0x57), // octobre  — or
    Rgb::new(0x63, 0xc7, 0xb2), // novembre — jade
    Rgb::new(0x93, 0xb7, 0xff), // décembre — bleu glacé
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(Rgb::parse_hex("#6ea8fe"), Some(Rgb::new(110, 168, 254)));
        assert_eq!(Rgb::parse_hex("6ea8fe"), Some(Rgb::new(110, 168, 254)));
        assert_eq!(Rgb::parse_hex("#abc"), Some(Rgb::new(170, 187, 204)));
        assert_eq!(Rgb::parse_hex("nope"), None);
        assert_eq!(Rgb::new(1, 2, 3).to_hex(), "#010203");
    }

    #[test]
    fn blending() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(100, 100, 100);
        assert_eq!(a.blend(b, 0), a);
        assert_eq!(a.blend(b, 100), b);
        assert_eq!(a.blend(b, 50), Rgb::new(50, 50, 50));
    }

    #[test]
    fn readable_text_chooses_contrast() {
        assert_eq!(
            Rgb::new(255, 255, 255).readable_text(),
            Rgb::new(16, 16, 20)
        );
        assert_eq!(Rgb::new(10, 10, 10).readable_text(), Rgb::WHITE);
    }

    #[test]
    fn ansi256_bounds() {
        assert!(Rgb::WHITE.to_ansi256() >= 231);
        assert!(Rgb::BLACK.to_ansi256() <= 16);
        assert!((16..=255).contains(&Rgb::new(110, 168, 254).to_ansi256()));
    }
}
