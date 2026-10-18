//! The reference sinks, each behind its own feature but `memory`.

#[cfg(feature = "console")]
mod console;
#[cfg(feature = "file")]
mod file;
#[cfg(feature = "loki")]
mod loki;
#[cfg(feature = "mail")]
mod mail;
mod memory;

#[cfg(feature = "console")]
pub use console::Console;
#[cfg(feature = "file")]
pub use file::{File, Rotate};
#[cfg(feature = "loki")]
pub use loki::Loki;
#[cfg(feature = "mail")]
pub use mail::{Mail, Security};
pub use memory::Memory;
