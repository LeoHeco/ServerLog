//! What can go wrong: at start-up, while registering a sink, and inside a sink.

use std::{fmt, io};

/// Why serlog could not start, a sink could not be registered, or a sink could not write.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// `init` ran a second time.
    Twice,
    /// A file or socket failed.
    Io(io::Error),
    /// A setting is missing or malformed.
    Config(String),
    /// A config file names a sink kind nobody registered.
    Kind(String),
    /// Two sinks share a name, or a name is reserved.
    Name(String),
    /// A resource, such as a file, is already claimed by another sink.
    Claimed(String),
    /// A sink failed its start-up check.
    NotReady {
        /// The sink's name.
        sink: String,
        /// What failed.
        cause: Box<Self>,
    },
    /// The other end refused the records; sending them again will not help.
    Rejected(String),
    /// Anything else a sink reports.
    Other(Box<dyn std::error::Error + Send + Sync>),
}

/// The result sinks and start-up return.
pub type Result<T = (), E = Error> = std::result::Result<T, E>;

impl Error {
    /// Wraps any error a sink runs into.
    pub fn other(e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self::Other(e.into())
    }

    /// Whether trying again could succeed.
    #[must_use]
    pub const fn is_transient(&self) -> bool {
        !matches!(self, Self::Rejected(_) | Self::Config(_))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Twice => f.write_str("serlog is already initialised"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Config(e) => write!(f, "bad config: {e}"),
            Self::Kind(k) => write!(f, "unknown sink kind `{k}`"),
            Self::Name(n) => write!(f, "sink name `{n}` is taken or reserved"),
            Self::Claimed(r) => write!(f, "{r} is already used by another sink"),
            Self::NotReady { sink, cause } => write!(f, "sink `{sink}` is not ready: {cause}"),
            Self::Rejected(e) => write!(f, "rejected: {e}"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::NotReady { cause, .. } => Some(cause.as_ref()),
            Self::Other(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<crate::ParseFilterError> for Error {
    fn from(e: crate::ParseFilterError) -> Self {
        Self::Config(e.to_string())
    }
}

impl From<crate::layout::PatternError> for Error {
    fn from(e: crate::layout::PatternError) -> Self {
        Self::Config(e.to_string())
    }
}

impl From<fmt::Error> for Error {
    fn from(e: fmt::Error) -> Self {
        Self::other(e)
    }
}
