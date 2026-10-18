//! A modern, high-performance logging framework: any number of sinks, each configured on its own.
//!
//! Records pass the hub's filter and processors, then go to every slot on the log route; each slot
//! has its own filter, processors, layout, queue, retries, health and failure policy, and a failing
//! sink raises an incident on the alarm route instead of disappearing.
//!
//! ```no_run
//! # #[cfg(all(feature = "console", feature = "file"))]
//! # fn main() -> serlog::Result {
//! use serlog::{Level, SinkExt, sinks::{Console, File, Rotate}};
//!
//! let _log = serlog::builder()
//!     .app("mirror")
//!     .sink(Console::new())
//!     .sink(File::new("logs/app.log").rotate(Rotate::Day).keep(7).gzip().name("app"))
//!     .sink(File::new("logs/errors.log").at(Level::ERROR))
//!     .init()?;
//! serlog::info!(files = 42, "synced");
//! # Ok(())
//! # }
//! # #[cfg(not(all(feature = "console", feature = "file")))]
//! # fn main() {}
//! ```
//!
//! With no setup at all it prints `info` and above to standard output, like most loggers.

pub mod alarm;
mod builder;
mod compat;
mod error;
mod filter;
mod guard;
mod hub;
pub mod layout;
mod level;
mod macros;
mod meta;
pub mod process;
mod record;
mod scope;
mod sink;
pub mod sinks;
mod slot;
#[cfg(any(feature = "loki", feature = "mail"))]
mod tls;
mod value;

#[cfg(feature = "config")]
pub mod config;

#[cfg(any(feature = "log", feature = "tracing"))]
mod bridge;

pub use alarm::{Alarm, Failure, Fallback, Incident, OnFail, Phase, Tiers, set_alarm};
pub use builder::{Builder, Guard, builder, init};
pub use compat::{Compat, Stream, UnknownCompat, compat};
pub use error::{Error, Result};
pub use filter::{Filter, ParseFilterError};
pub use hub::{
    add, catch_panics, emit, enabled, filter, flush, process, redact, remove, set_filter,
    set_level, set_process, status,
};
pub use layout::{Colour, Layout};
pub use level::{Level, ParseLevelError};
pub use meta::{Meta, meta};
pub use record::{Caller, Field, Msg, Record, RecordBuf};
pub use scope::Bound;
pub use sink::{Entry, FromFn, Sink, Written, from_fn};
pub use slot::{Full, Health, Queue, SinkExt, Slot, Status};
pub use value::{ErrorBuf, List, Map, ToValue, Val, Value, err, group};

#[cfg(feature = "log")]
pub use bridge::Bridge;
#[cfg(feature = "tracing")]
pub use bridge::Layer;

#[doc(hidden)]
pub mod __private {
    pub use crate::{
        hub::log,
        macros::func,
        slot::{Breaker, Tell},
    };
}
