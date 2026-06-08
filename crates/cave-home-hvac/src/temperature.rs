//! Temperature value object — canonical Celsius with validated range.
//!
//! cave-home stores every temperature in Celsius internally (the Charter §7
//! single-source-of-truth rule), exactly like Home Assistant's climate domain
//! keeps a canonical unit and converts at the edges. A vendor thermostat that
//! reports Fahrenheit is converted on the way in; the Portal renders the user's
//! preferred unit on the way out. Everything in between reasons in Celsius.

/// The widest physically-sensible indoor/outdoor band cave-home will accept for
/// a household climate reading. Anything outside this is a sensor fault, not a
/// temperature, and is rejected at construction.
pub const MIN_CELSIUS: f64 = -50.0;
/// Upper bound of the accepted band (see [`MIN_CELSIUS`]).
pub const MAX_CELSIUS: f64 = 60.0;

/// A validated temperature, stored canonically in degrees Celsius.
///
/// Construction rejects non-finite values and anything outside
/// [`MIN_CELSIUS`]`..=`[`MAX_CELSIUS`], so downstream control logic never has to
/// defend against `NaN` or absurd readings.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Temperature {
    celsius: f64,
}

/// Why a [`Temperature`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemperatureError {
    /// The value was `NaN` or infinite.
    NotFinite,
    /// The value was outside the accepted `-50..=60 °C` band.
    OutOfRange,
}

impl core::fmt::Display for TemperatureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFinite => f.write_str("temperature value is not finite"),
            Self::OutOfRange => f.write_str("temperature value is out of the accepted range"),
        }
    }
}

impl std::error::Error for TemperatureError {}

impl Temperature {
    /// Construct a validated temperature from a Celsius value.
    ///
    /// # Errors
    /// Returns [`TemperatureError`] if `celsius` is non-finite or outside
    /// [`MIN_CELSIUS`]`..=`[`MAX_CELSIUS`].
    pub fn from_celsius(celsius: f64) -> Result<Self, TemperatureError> {
        if !celsius.is_finite() {
            return Err(TemperatureError::NotFinite);
        }
        if celsius < MIN_CELSIUS || celsius > MAX_CELSIUS {
            return Err(TemperatureError::OutOfRange);
        }
        Ok(Self { celsius })
    }

    /// Construct a validated temperature from a Fahrenheit value.
    ///
    /// The value is converted to canonical Celsius and then range-checked, so a
    /// thermostat that speaks Fahrenheit is accepted on exactly the same terms.
    ///
    /// # Errors
    /// Returns [`TemperatureError`] if the converted value is non-finite or out
    /// of range.
    pub fn from_fahrenheit(fahrenheit: f64) -> Result<Self, TemperatureError> {
        Self::from_celsius(fahrenheit_to_celsius(fahrenheit))
    }

    /// The canonical value in degrees Celsius.
    #[must_use]
    pub const fn celsius(self) -> f64 {
        self.celsius
    }

    /// The value converted to degrees Fahrenheit (for display only).
    #[must_use]
    pub fn fahrenheit(self) -> f64 {
        celsius_to_fahrenheit(self.celsius)
    }
}

/// Convert Celsius to Fahrenheit: `°F = °C × 9/5 + 32`.
#[must_use]
pub fn celsius_to_fahrenheit(celsius: f64) -> f64 {
    celsius * 9.0 / 5.0 + 32.0
}

/// Convert Fahrenheit to Celsius: `°C = (°F − 32) × 5/9`.
#[must_use]
pub fn fahrenheit_to_celsius(fahrenheit: f64) -> f64 {
    (fahrenheit - 32.0) * 5.0 / 9.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nan_and_infinite() {
        assert_eq!(
            Temperature::from_celsius(f64::NAN),
            Err(TemperatureError::NotFinite)
        );
        assert_eq!(
            Temperature::from_celsius(f64::INFINITY),
            Err(TemperatureError::NotFinite)
        );
    }

    #[test]
    fn rejects_out_of_range() {
        assert_eq!(
            Temperature::from_celsius(-50.1),
            Err(TemperatureError::OutOfRange)
        );
        assert_eq!(
            Temperature::from_celsius(60.1),
            Err(TemperatureError::OutOfRange)
        );
    }

    #[test]
    fn accepts_range_endpoints() {
        assert!(Temperature::from_celsius(MIN_CELSIUS).is_ok());
        assert!(Temperature::from_celsius(MAX_CELSIUS).is_ok());
        assert!(Temperature::from_celsius(21.0).is_ok());
    }

    #[test]
    fn celsius_fahrenheit_reference_points() {
        // Freezing and boiling, the textbook anchors.
        assert!((celsius_to_fahrenheit(0.0) - 32.0).abs() < 1e-9);
        assert!((celsius_to_fahrenheit(100.0) - 212.0).abs() < 1e-9);
        // -40 is the crossover where the two scales meet.
        assert!((celsius_to_fahrenheit(-40.0) - (-40.0)).abs() < 1e-9);
        // Body temperature, a common thermostat-adjacent value.
        assert!((fahrenheit_to_celsius(98.6) - 37.0).abs() < 1e-9);
    }

    #[test]
    fn conversion_round_trips() {
        for c in [-40.0, -10.0, 0.0, 18.5, 21.0, 37.0, 55.0] {
            let back = fahrenheit_to_celsius(celsius_to_fahrenheit(c));
            assert!((back - c).abs() < 1e-9, "round-trip failed for {c}");
        }
    }

    #[test]
    fn from_fahrenheit_stores_canonical_celsius() {
        let t = Temperature::from_fahrenheit(68.0).expect("68 °F is in range");
        assert!((t.celsius() - 20.0).abs() < 1e-9);
        assert!((t.fahrenheit() - 68.0).abs() < 1e-9);
    }

    #[test]
    fn from_fahrenheit_range_checks_after_conversion() {
        // 200 °F ≈ 93 °C, above the accepted band.
        assert_eq!(
            Temperature::from_fahrenheit(200.0),
            Err(TemperatureError::OutOfRange)
        );
    }
}
