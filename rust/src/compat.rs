//! Profiles that make serlog behave like another logger: its layout, stream, default level and variable.

use std::{
    fmt,
    str::FromStr,
    sync::atomic::{AtomicU8, Ordering},
};

use crate::{Level, layout::preset};

/// Another logger whose defaults serlog can take on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Compat {
    /// serlog's own.
    #[default]
    Serlog,
    /// `tracing-subscriber`'s `fmt` layer.
    Tracing,
    /// `env_logger`.
    EnvLogger,
    /// Python's `logging.basicConfig`.
    Python,
    /// Logback and Log4j 2.
    Logback,
    /// Go's `slog.TextHandler`.
    Slog,
    /// `pino` with `pino-pretty`.
    Pino,
}

/// Where a console writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Stream {
    /// Standard output.
    #[default]
    Stdout,
    /// Standard error.
    Stderr,
    /// Errors to standard error, the rest to standard output.
    Auto,
}

impl Compat {
    /// Every profile.
    pub const ALL: [Self; 7] = [
        Self::Serlog,
        Self::Tracing,
        Self::EnvLogger,
        Self::Python,
        Self::Logback,
        Self::Slog,
        Self::Pino,
    ];

    /// The name used in config files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Serlog => "serlog",
            Self::Tracing => "tracing",
            Self::EnvLogger => "env_logger",
            Self::Python => "python",
            Self::Logback => "logback",
            Self::Slog => "slog",
            Self::Pino => "pino",
        }
    }

    /// The line pattern.
    #[must_use]
    pub const fn pattern(self) -> &'static str {
        match self {
            Self::Serlog => preset::SERLOG,
            Self::Tracing => preset::TRACING,
            Self::EnvLogger => preset::ENV_LOGGER,
            Self::Python => preset::PYTHON,
            Self::Logback => preset::LOGBACK,
            Self::Slog => preset::SLOG,
            Self::Pino => preset::PINO,
        }
    }

    /// Where the console writes.
    #[must_use]
    pub const fn stream(self) -> Stream {
        match self {
            Self::EnvLogger | Self::Python => Stream::Stderr,
            _ => Stream::Stdout,
        }
    }

    /// The level when nothing else is set.
    #[must_use]
    pub const fn level(self) -> Level {
        match self {
            Self::EnvLogger => Level::ERROR,
            Self::Python => Level::WARN,
            Self::Logback => Level::DEBUG,
            _ => Level::INFO,
        }
    }

    /// Environment variables read for the filter, first set wins.
    #[must_use]
    pub const fn env(self) -> &'static [&'static str] {
        match self {
            Self::Serlog => &["SERLOG_FILTER", "RUST_LOG"],
            Self::Tracing | Self::EnvLogger => &["RUST_LOG"],
            _ => &[],
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Compat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A profile name nobody knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownCompat(pub String);

impl fmt::Display for UnknownCompat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown compat profile `{}`", self.0)
    }
}

impl std::error::Error for UnknownCompat {}

impl FromStr for Compat {
    type Err = UnknownCompat;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "log4j" => Ok(Self::Logback),
            _ => Self::ALL
                .into_iter()
                .find(|c| c.name() == s)
                .ok_or_else(|| UnknownCompat(s.to_owned())),
        }
    }
}

impl FromStr for Stream {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stdout" => Ok(Self::Stdout),
            "stderr" => Ok(Self::Stderr),
            "auto" => Ok(Self::Auto),
            _ => Err(format!("unknown stream `{s}`")),
        }
    }
}

impl Stream {
    /// The name used in config files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::Auto => "auto",
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The profile in force.
#[must_use]
pub fn compat() -> Compat {
    Compat::ALL[usize::from(CURRENT.load(Ordering::Relaxed)).min(Compat::ALL.len() - 1)]
}

pub(crate) fn set(c: Compat) {
    #[expect(clippy::cast_possible_truncation, reason = "seven profiles")]
    CURRENT.store(c.index() as u8, Ordering::Relaxed);
}
