// SPDX-License-Identifier: Apache-2.0
//! Theme / branding model + responsive breakpoint hints.
//!
//! Pure layout maths and a small palette — no rendering. The (deferred)
//! frontend turns a [`Theme`] into CSS variables and uses [`Breakpoint`] to
//! pick a column count for the current viewport width.

/// A named viewport class. Boundaries are first-party defaults chosen for a
/// touch-first home dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakpoint {
    /// Phone-width (the mobile companion or a narrow Portal window).
    Mobile,
    /// Tablet-width (the common kitchen wall-panel).
    Tablet,
    /// Desktop / large wall-display width.
    Desktop,
}

impl Breakpoint {
    /// Classify a viewport width (CSS pixels) into a breakpoint.
    /// `< 600` mobile, `< 1024` tablet, otherwise desktop.
    #[must_use]
    pub const fn from_width(px: u32) -> Self {
        if px < 600 {
            Self::Mobile
        } else if px < 1024 {
            Self::Tablet
        } else {
            Self::Desktop
        }
    }

    /// How many dashboard card columns to render at this breakpoint. One column
    /// on a phone keeps tap-targets large for the headline persona.
    #[must_use]
    pub const fn columns(self) -> u8 {
        match self {
            Self::Mobile => 1,
            Self::Tablet => 2,
            Self::Desktop => 4,
        }
    }
}

/// Light vs dark base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Light base.
    Light,
    /// Dark base.
    Dark,
}

/// A theme: a base mode, an accent colour, and a home/brand name shown in the
/// header. Colours are validated hex strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Display name in the header ("Our home").
    pub brand: String,
    /// Light or dark base.
    pub mode: Mode,
    /// Accent colour as a `#rrggbb` hex string.
    accent: String,
}

/// Why a theme could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeError {
    /// The accent colour was not a `#rrggbb` string.
    BadAccentHex,
    /// The brand name was empty or whitespace.
    EmptyBrand,
}

impl Theme {
    /// Build a theme, validating the accent hex (`#rrggbb`) and brand name.
    ///
    /// # Errors
    /// Returns [`ThemeError`] if the accent is not a 6-digit hex colour or the
    /// brand name is blank.
    pub fn new(
        brand: impl Into<String>,
        mode: Mode,
        accent: impl Into<String>,
    ) -> Result<Self, ThemeError> {
        let brand = brand.into();
        if brand.trim().is_empty() {
            return Err(ThemeError::EmptyBrand);
        }
        let accent = accent.into();
        if !is_hex_color(&accent) {
            return Err(ThemeError::BadAccentHex);
        }
        Ok(Self {
            brand,
            mode,
            accent,
        })
    }

    /// The validated accent colour.
    #[must_use]
    pub fn accent(&self) -> &str {
        &self.accent
    }

    /// The default cave-home theme: a friendly dark base with a warm accent.
    #[must_use]
    pub fn cave_home_default() -> Self {
        Self {
            brand: "Our home".to_string(),
            mode: Mode::Dark,
            accent: "#ffb000".to_string(),
        }
    }
}

fn is_hex_color(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints_classify_widths() {
        assert_eq!(Breakpoint::from_width(360), Breakpoint::Mobile);
        assert_eq!(Breakpoint::from_width(599), Breakpoint::Mobile);
        assert_eq!(Breakpoint::from_width(600), Breakpoint::Tablet);
        assert_eq!(Breakpoint::from_width(1023), Breakpoint::Tablet);
        assert_eq!(Breakpoint::from_width(1024), Breakpoint::Desktop);
        assert_eq!(Breakpoint::from_width(2560), Breakpoint::Desktop);
    }

    #[test]
    fn columns_grow_with_width() {
        assert_eq!(Breakpoint::Mobile.columns(), 1);
        assert_eq!(Breakpoint::Tablet.columns(), 2);
        assert_eq!(Breakpoint::Desktop.columns(), 4);
        assert!(Breakpoint::Mobile.columns() < Breakpoint::Desktop.columns());
    }

    #[test]
    fn valid_theme_builds() {
        let t = Theme::new("Our home", Mode::Dark, "#ffb000").expect("valid");
        assert_eq!(t.brand, "Our home");
        assert_eq!(t.accent(), "#ffb000");
        assert_eq!(t.mode, Mode::Dark);
    }

    #[test]
    fn bad_accent_rejected() {
        assert_eq!(
            Theme::new("Home", Mode::Light, "ffb000"),
            Err(ThemeError::BadAccentHex)
        );
        assert_eq!(
            Theme::new("Home", Mode::Light, "#fff"),
            Err(ThemeError::BadAccentHex)
        );
        assert_eq!(
            Theme::new("Home", Mode::Light, "#gggggg"),
            Err(ThemeError::BadAccentHex)
        );
    }

    #[test]
    fn blank_brand_rejected() {
        assert_eq!(
            Theme::new("  ", Mode::Light, "#ffffff"),
            Err(ThemeError::EmptyBrand)
        );
    }

    #[test]
    fn default_theme_is_valid() {
        let t = Theme::cave_home_default();
        assert!(is_hex_color(t.accent()));
        assert!(!t.brand.is_empty());
    }
}
