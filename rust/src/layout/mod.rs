//! How a record becomes text: a pattern, logfmt, a JSON schema, the pretty layout, or a closure.

mod colour;
mod json;
mod pattern;
mod text;
pub(crate) mod time;

use std::sync::{Arc, LazyLock};

pub use colour::{Colour, Theme, When};
pub use json::{Json, Levels, Schema, value as json_value};
pub use pattern::{Pattern, PatternError};
#[cfg(feature = "loki")]
pub(crate) use text::flat;
pub use text::quote;

use crate::{Compat, Record, compat};

/// Patterns that reproduce other loggers' default output.
pub mod preset {
    /// serlog's own default.
    pub const SERLOG: &str = "{time} {level:<5} {target}: {msg}{fields}";
    /// `tracing-subscriber`'s `fmt` layer with local RFC 3339 time.
    pub const TRACING: &str = "{time:rfc3339} {level:>5} {spans}{target}: {msg}{fields}";
    /// `env_logger`.
    pub const ENV_LOGGER: &str = "[{time:utc} {level:<5} {target}] {msg}";
    /// Python's `logging.basicConfig`.
    pub const PYTHON: &str = "{level}:{target}:{msg}";
    /// Logback and Log4j 2 console defaults.
    pub const LOGBACK: &str = "{time:%H:%M:%S%.3f} [{thread}] {level:<5} {target:~36} - {msg}";
    /// Go's `slog.TextHandler`.
    pub const SLOG: &str =
        "time={time:%Y-%m-%dT%H:%M:%S%.3f%:z} level={level} msg={msg:q}{fields:q}";
    /// `pino-pretty`.
    pub const PINO: &str = "[{time:%H:%M:%S%.3f}] {level} ({pid}): {msg}{fields}";

    /// The pattern behind a preset name, if it is one.
    #[must_use]
    pub fn named(name: &str) -> Option<&'static str> {
        Some(match name {
            "serlog" => SERLOG,
            "tracing" => TRACING,
            "env_logger" => ENV_LOGGER,
            "python" => PYTHON,
            "logback" | "log4j" => LOGBACK,
            "slog" => SLOG,
            "pino" => PINO,
            _ => return None,
        })
    }
}

/// A caller-supplied layout: appends one record to the line; the theme is set when colouring.
pub type Render = dyn Fn(&mut String, &Record<'_>, Option<&Theme>) + Send + Sync;

/// How a record becomes one line.
#[derive(Clone, Default)]
#[non_exhaustive]
pub enum Layout {
    /// The pattern of the compat profile in force.
    #[default]
    Inherit,
    /// A [`Pattern`].
    Pattern(Arc<Pattern>),
    /// logfmt.
    Logfmt,
    /// A [`Json`] schema.
    Json(Arc<Json>),
    /// A first line plus one line per field, for reading in a terminal.
    Pretty,
    /// Anything else, written by the caller.
    Custom(Arc<Render>),
}

impl std::fmt::Debug for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inherit => f.write_str("Inherit"),
            Self::Pattern(p) => f.debug_tuple("Pattern").field(p).finish(),
            Self::Logfmt => f.write_str("Logfmt"),
            Self::Json(j) => f.debug_tuple("Json").field(j).finish(),
            Self::Pretty => f.write_str("Pretty"),
            Self::Custom(_) => f.write_str("Custom"),
        }
    }
}

static INHERITED: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    Compat::ALL
        .iter()
        .map(|c| c.pattern().parse().unwrap_or_default())
        .collect()
});

impl Layout {
    /// A layout written by `f`.
    pub fn custom(
        f: impl Fn(&mut String, &Record<'_>, Option<&Theme>) + Send + Sync + 'static,
    ) -> Self {
        Self::Custom(Arc::new(f))
    }

    /// A pattern, or a preset name such as `logback`.
    ///
    /// # Errors
    /// When the pattern does not parse.
    pub fn pattern(pattern: &str) -> Result<Self, PatternError> {
        Ok(Self::Pattern(Arc::new(pattern.parse()?)))
    }

    /// JSON in `schema`.
    #[must_use]
    pub fn json(schema: Schema) -> Self {
        Self::Json(Arc::new(Json::new(schema)))
    }

    /// Appends `rec` to `out`, without a line break; a theme colours it.
    pub fn render(&self, out: &mut String, rec: &Record<'_>, theme: Option<&Theme>) {
        let _ = match self {
            Self::Inherit => {
                let c = compat();
                INHERITED[Compat::ALL.iter().position(|x| *x == c).unwrap_or(0)]
                    .write(out, rec, theme)
            }
            Self::Pattern(p) => p.write(out, rec, theme),
            Self::Logfmt => text::render_logfmt(out, rec),
            Self::Json(j) => j.write(out, rec),
            Self::Pretty => text::render_pretty(out, rec, theme),
            Self::Custom(f) => {
                f(out, rec, theme);
                Ok(())
            }
        };
    }

    /// Identity for the per-record render cache: equal ids render equally.
    pub(crate) fn id(&self) -> usize {
        match self {
            Self::Inherit => 1,
            Self::Logfmt => 2,
            Self::Pretty => 3,
            Self::Pattern(p) => Arc::as_ptr(p).addr(),
            Self::Json(j) => Arc::as_ptr(j).addr(),
            Self::Custom(f) => Arc::as_ptr(f).cast::<()>().addr(),
        }
    }
}

impl From<Pattern> for Layout {
    fn from(p: Pattern) -> Self {
        Self::Pattern(Arc::new(p))
    }
}

impl From<Json> for Layout {
    fn from(j: Json) -> Self {
        Self::Json(Arc::new(j))
    }
}

impl From<Schema> for Layout {
    fn from(s: Schema) -> Self {
        Self::json(s)
    }
}
