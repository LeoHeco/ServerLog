//! Severity: six built-in levels from 10 to 60, plus any defined at run time.

use std::{
    fmt,
    str::FromStr,
    sync::{PoisonError, RwLock},
};

use crate::{Compat, compat, layout::Colour};

/// How severe a record is; only the number counts when comparing.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Level(u8);

struct Custom {
    value: u8,
    name: &'static str,
    upper: &'static str,
    colour: Colour,
}

static CUSTOM: RwLock<Vec<Custom>> = RwLock::new(Vec::new());

impl Level {
    /// Step-by-step detail.
    pub const TRACE: Self = Self(10);
    /// Diagnostics for developers.
    pub const DEBUG: Self = Self(20);
    /// Normal operation.
    pub const INFO: Self = Self(30);
    /// Something unexpected that the program survived.
    pub const WARN: Self = Self(40);
    /// A failure that needs attention.
    pub const ERROR: Self = Self(50);
    /// A failure the program cannot carry on from.
    pub const FATAL: Self = Self(60);

    /// The built-in levels, lowest first.
    pub const BUILT_IN: [Self; 6] = [
        Self::TRACE,
        Self::DEBUG,
        Self::INFO,
        Self::WARN,
        Self::ERROR,
        Self::FATAL,
    ];

    /// The level with this number, named or not.
    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    /// Its number.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }

    /// Defines a level, or renames the one with that number.
    ///
    /// # Errors
    /// When `name` is empty, not lower-case ASCII, or already a built-in name.
    pub fn define(name: &str, value: u8, colour: Colour) -> Result<Self, ParseLevelError> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || Self::BUILT_IN.iter().any(|l| l.builtin() == Some(name))
            || name == "off"
        {
            return Err(ParseLevelError);
        }
        let mut all = CUSTOM.write().unwrap_or_else(PoisonError::into_inner);
        all.retain(|c| c.value != value && c.name != name);
        all.push(Custom {
            value,
            name: Box::leak(name.into()),
            upper: Box::leak(name.to_ascii_uppercase().into()),
            colour,
        });
        Ok(Self(value))
    }

    const fn builtin(self) -> Option<&'static str> {
        Some(match self.0 {
            10 => "trace",
            20 => "debug",
            30 => "info",
            40 => "warn",
            50 => "error",
            60 => "fatal",
            _ => return None,
        })
    }

    fn custom<T>(self, f: impl FnOnce(&Custom) -> T) -> Option<T> {
        CUSTOM
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|c| c.value == self.0)
            .map(f)
    }

    /// Every level defined at run time, as name, value and colour.
    #[cfg(feature = "config")]
    pub(crate) fn defined() -> Vec<(&'static str, u8, Colour)> {
        CUSTOM
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|c| (c.name, c.value, c.colour))
            .collect()
    }

    /// The lower-case name used in config files, filters and labels.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.builtin()
            .or_else(|| self.custom(|c| c.name))
            .unwrap_or("custom")
    }

    /// How a profile prints it, such as `WARNING` for Python's.
    #[must_use]
    pub fn label(self, profile: Compat) -> &'static str {
        match (profile, self.0) {
            (Compat::Python, 40) => "WARNING",
            (Compat::Python, 60) => "CRITICAL",
            (Compat::Slog, 10) => "DEBUG-4",
            (Compat::Slog, 60) => "ERROR+4",
            (_, 10) => "TRACE",
            (_, 20) => "DEBUG",
            (_, 30) => "INFO",
            (_, 40) => "WARN",
            (_, 50) => "ERROR",
            (_, 60) => "FATAL",
            _ => self.custom(|c| c.upper).unwrap_or("CUSTOM"),
        }
    }

    /// Its colour in a terminal.
    #[must_use]
    pub fn colour(self) -> Colour {
        match self.0 {
            10 => Colour::Cyan,
            20 => Colour::Magenta,
            30 => Colour::Green,
            40 => Colour::Yellow,
            50 => Colour::Red,
            60 => Colour::Alarm,
            _ => self.custom(|c| c.colour).unwrap_or(Colour::Plain),
        }
    }

    /// The syslog severity, for GELF and syslog-shaped outputs.
    #[must_use]
    pub const fn syslog(self) -> u8 {
        match self.0 {
            0..=20 => 7,
            21..=30 => 6,
            31..=40 => 4,
            41..=50 => 3,
            _ => 2,
        }
    }
}

impl Default for Level {
    fn default() -> Self {
        Self::INFO
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.label(compat()))
    }
}

impl fmt::Debug for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.name(), self.0)
    }
}

/// A level name nobody defined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseLevelError;

impl fmt::Display for ParseLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected trace, debug, info, warn, error, fatal, a defined level or a number")
    }
}

impl std::error::Error for ParseLevelError {}

impl FromStr for Level {
    type Err = ParseLevelError;

    /// Any case; also `warning`, `critical` and plain numbers.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().to_ascii_lowercase();
        if let Ok(n) = s.parse::<u8>() {
            return Ok(Self(n));
        }
        match s.as_str() {
            "warning" => return Ok(Self::WARN),
            "critical" => return Ok(Self::FATAL),
            _ => {}
        }
        Self::BUILT_IN
            .into_iter()
            .find(|l| l.builtin() == Some(s.as_str()))
            .or_else(|| {
                CUSTOM
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .iter()
                    .find(|c| c.name == s)
                    .map(|c| Self(c.value))
            })
            .ok_or(ParseLevelError)
    }
}

#[cfg(feature = "config")]
impl<'de> serde::Deserialize<'de> for Level {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::Level;
    use crate::{Compat, layout::Colour};

    #[test]
    fn parses_names_numbers_and_custom_levels() {
        assert_eq!("WaRn".parse(), Ok(Level::WARN));
        assert_eq!("warning".parse(), Ok(Level::WARN));
        assert_eq!("critical".parse(), Ok(Level::FATAL));
        assert_eq!("35".parse(), Ok(Level::new(35)));
        assert!("loud".parse::<Level>().is_err());
        let notice = Level::define("notice", 35, Colour::Blue).unwrap();
        assert_eq!("notice".parse(), Ok(notice));
        assert_eq!(notice.label(Compat::Serlog), "NOTICE");
        assert!(Level::INFO < notice && notice < Level::WARN);
        assert!(Level::define("info", 31, Colour::Plain).is_err());
        assert_eq!(Level::WARN.label(Compat::Python), "WARNING");
        assert_eq!(Level::TRACE.label(Compat::Slog), "DEBUG-4");
    }
}
