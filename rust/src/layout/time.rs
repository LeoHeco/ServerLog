//! How a record's time is written.

use std::{
    fmt::{self, Write},
    sync::LazyLock,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Starts the clock behind `{elapsed}`.
pub(crate) fn start() {
    LazyLock::force(&START);
}

/// Milliseconds since serlog started.
pub(crate) fn elapsed() -> u128 {
    START.elapsed().as_millis()
}

/// A time format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Clock {
    Local,
    Utc,
    Rfc3339,
    Unix,
    UnixMs,
    UnixNs,
    Custom(String),
}

impl Clock {
    pub(crate) fn parse(spec: &str) -> Self {
        match spec {
            "" | "local" => Self::Local,
            "utc" => Self::Utc,
            "rfc3339" => Self::Rfc3339,
            "unix" => Self::Unix,
            "unix_ms" => Self::UnixMs,
            "unix_ns" => Self::UnixNs,
            f => Self::Custom(f.to_owned()),
        }
    }

    pub(crate) fn write(&self, w: &mut (impl Write + ?Sized), t: SystemTime) -> fmt::Result {
        let since = t.duration_since(UNIX_EPOCH).unwrap_or_default();
        match self {
            Self::Local => write!(w, "{}", local(t).strftime("%Y-%m-%d %H:%M:%S%.3f")),
            Self::Utc => write!(w, "{:.0}", stamp(t)),
            Self::Rfc3339 => write!(w, "{}", local(t).strftime("%Y-%m-%dT%H:%M:%S%.6f%:z")),
            Self::Unix => write!(w, "{}", since.as_secs()),
            Self::UnixMs => write!(w, "{}", since.as_millis()),
            Self::UnixNs => write!(w, "{}", since.as_nanos()),
            Self::Custom(f) => write!(w, "{}", local(t).strftime(f)),
        }
    }
}

pub(crate) fn stamp(t: SystemTime) -> jiff::Timestamp {
    jiff::Timestamp::try_from(t).unwrap_or(jiff::Timestamp::UNIX_EPOCH)
}

pub(crate) fn local(t: SystemTime) -> jiff::Zoned {
    stamp(t).to_zoned(jiff::tz::TimeZone::system())
}

/// RFC 3339 in UTC with milliseconds, as most JSON schemas want.
pub(crate) fn iso(t: SystemTime) -> impl fmt::Display {
    struct Iso(jiff::Timestamp);
    impl fmt::Display for Iso {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{:.3}", self.0)
        }
    }
    Iso(stamp(t))
}

/// Seconds since the epoch with a fraction, as zap and GELF want.
pub(crate) fn unix_float(t: SystemTime) -> f64 {
    t.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
