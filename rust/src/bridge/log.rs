//! Serlog as the backend of the `log` crate.

use ::log::{LevelFilter, Log, Metadata, SetLoggerError};

use crate::{Caller, Level, Record};

/// Forwards `log` records to serlog.
#[derive(Clone, Copy, Debug, Default)]
pub struct Bridge;

static BRIDGE: Bridge = Bridge;

impl Bridge {
    /// Makes serlog receive everything logged through the `log` macros.
    ///
    /// # Errors
    /// Fails if another `log` backend was installed first.
    pub fn install() -> Result<(), SetLoggerError> {
        ::log::set_logger(&BRIDGE)?;
        ::log::set_max_level(LevelFilter::Trace);
        Ok(())
    }
}

const fn level(l: ::log::Level) -> Level {
    match l {
        ::log::Level::Trace => Level::TRACE,
        ::log::Level::Debug => Level::DEBUG,
        ::log::Level::Info => Level::INFO,
        ::log::Level::Warn => Level::WARN,
        ::log::Level::Error => Level::ERROR,
    }
}

impl Log for Bridge {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        crate::enabled(level(m.level()))
    }

    fn log(&self, r: &::log::Record<'_>) {
        let lvl = level(r.level());
        if !crate::enabled(lvl) {
            return;
        }
        crate::emit(&Record::new(lvl, r.target(), *r.args()).caller(Caller {
            module: r.module_path(),
            func: None,
            file: r.file(),
            line: r.line(),
        }));
    }

    fn flush(&self) {
        crate::flush();
    }
}
