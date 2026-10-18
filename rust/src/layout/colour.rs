//! Terminal colours, per level and overridable, honouring `NO_COLOR` and `FORCE_COLOR`.

use std::{fmt, str::FromStr};

use crate::Level;

/// A terminal colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Colour {
    /// No colour.
    #[default]
    Plain,
    /// Red.
    Red,
    /// Green.
    Green,
    /// Yellow.
    Yellow,
    /// Blue.
    Blue,
    /// Magenta.
    Magenta,
    /// Cyan.
    Cyan,
    /// White.
    White,
    /// Grey, for the less important parts of a line.
    Grey,
    /// White on red, for fatal records.
    Alarm,
}

impl Colour {
    /// The ANSI escape that starts it.
    #[must_use]
    pub const fn ansi(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Red => "\x1b[1;31m",
            Self::Green => "\x1b[32m",
            Self::Yellow => "\x1b[33m",
            Self::Blue => "\x1b[34m",
            Self::Magenta => "\x1b[35m",
            Self::Cyan => "\x1b[36m",
            Self::White => "\x1b[37m",
            Self::Grey => "\x1b[90m",
            Self::Alarm => "\x1b[1;37;41m",
        }
    }

    /// The name used in config files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Red => "red",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Blue => "blue",
            Self::Magenta => "magenta",
            Self::Cyan => "cyan",
            Self::White => "white",
            Self::Grey => "grey",
            Self::Alarm => "alarm",
        }
    }

    const ALL: [Self; 10] = [
        Self::Plain,
        Self::Red,
        Self::Green,
        Self::Yellow,
        Self::Blue,
        Self::Magenta,
        Self::Cyan,
        Self::White,
        Self::Grey,
        Self::Alarm,
    ];
}

impl FromStr for Colour {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = if s == "gray" { "grey" } else { s };
        Self::ALL
            .into_iter()
            .find(|c| c.name() == s)
            .ok_or_else(|| format!("unknown colour `{s}`"))
    }
}

impl fmt::Display for Colour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// When to colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum When {
    /// When writing to a terminal, unless `NO_COLOR` is set; always when `FORCE_COLOR` is.
    #[default]
    Auto,
    /// Always.
    Always,
    /// Never.
    Never,
}

impl When {
    /// Decides for a sink that is, or is not, a terminal.
    #[must_use]
    pub fn resolve(self, tty: bool) -> bool {
        let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Auto if set("FORCE_COLOR") => true,
            Self::Auto => tty && !set("NO_COLOR"),
        }
    }

    /// The name used in config files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

impl FromStr for When {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            "always" | "true" => Ok(Self::Always),
            "never" | "false" => Ok(Self::Never),
            _ => Err(format!("`colour` takes auto, always or never, not `{s}`")),
        }
    }
}

/// Colours by level; levels not listed keep their own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Theme(Vec<(Level, Colour)>);

impl Theme {
    /// Gives `level` another colour.
    #[must_use]
    pub fn set(mut self, level: Level, colour: Colour) -> Self {
        self.0.retain(|(l, _)| *l != level);
        self.0.push((level, colour));
        self
    }

    /// The colour for `level`.
    #[must_use]
    pub fn of(&self, level: Level) -> Colour {
        self.0
            .iter()
            .find(|(l, _)| *l == level)
            .map_or_else(|| level.colour(), |(_, c)| *c)
    }

    /// The overrides.
    #[must_use]
    pub fn overrides(&self) -> &[(Level, Colour)] {
        &self.0
    }
}

/// Wraps `text` in `colour` when `on`.
pub(crate) fn paint(out: &mut String, on: bool, colour: Colour, text: impl FnOnce(&mut String)) {
    if on && colour != Colour::Plain {
        out.push_str(colour.ansi());
        text(out);
        out.push_str("\x1b[0m");
    } else {
        text(out);
    }
}
