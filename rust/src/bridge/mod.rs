//! Feeds records from the `log` and `tracing` ecosystems into serlog.

#[cfg(feature = "log")]
mod log;
#[cfg(feature = "tracing")]
mod tracing;

#[cfg(feature = "log")]
pub use self::log::Bridge;
#[cfg(feature = "tracing")]
pub use self::tracing::Layer;
