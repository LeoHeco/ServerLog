//! Standard output or standard error, one line per entry.

use std::io::{self, IsTerminal, Write};

use crate::{Compat, Entry, Level, Result, Sink, Stream, compat};

/// Prints one line per entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Console {
    stream: Option<Stream>,
}

impl Console {
    /// Writes where the compat profile says, standard output by default.
    pub const fn new() -> Self {
        Self { stream: None }
    }

    /// Standard output.
    pub const fn stdout() -> Self {
        Self::on(Stream::Stdout)
    }

    /// Standard error.
    pub const fn stderr() -> Self {
        Self::on(Stream::Stderr)
    }

    /// Errors and worse to standard error, the rest to standard output.
    pub const fn auto() -> Self {
        Self::on(Stream::Auto)
    }

    /// Writes to `stream`.
    pub const fn on(stream: Stream) -> Self {
        Self {
            stream: Some(stream),
        }
    }

    fn stream(self) -> Stream {
        self.stream.unwrap_or_else(|| Compat::stream(compat()))
    }

    fn err(self, level: Level) -> bool {
        match self.stream() {
            Stream::Stdout => false,
            Stream::Stderr => true,
            Stream::Auto => level >= Level::ERROR,
        }
    }
}

fn put(err: bool, text: &str) -> Result {
    if err {
        io::stderr().lock().write_all(text.as_bytes())?;
    } else {
        io::stdout().lock().write_all(text.as_bytes())?;
    }
    Ok(())
}

impl Sink for Console {
    fn emit(&self, entry: &Entry<'_>) -> Result {
        let line = entry.line();
        let mut text = String::with_capacity(line.len() + 1);
        text.push_str(line);
        text.push('\n');
        put(self.err(entry.record().level()), &text)
    }

    fn emit_batch(&self, entries: &[Entry<'_>]) -> Result {
        let (mut out, mut err) = (String::new(), String::new());
        for e in entries {
            let to = if self.err(e.record().level()) {
                &mut err
            } else {
                &mut out
            };
            to.push_str(e.line());
            to.push('\n');
        }
        if !out.is_empty() {
            put(false, &out)?;
        }
        if !err.is_empty() {
            put(true, &err)?;
        }
        Ok(())
    }

    fn flush(&self) -> Result {
        io::stdout().flush()?;
        io::stderr().flush()?;
        Ok(())
    }

    fn kind(&self) -> &'static str {
        "console"
    }

    fn tty(&self) -> bool {
        match self.stream() {
            Stream::Stderr => io::stderr().is_terminal(),
            _ => io::stdout().is_terminal(),
        }
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        let mut m = serde_json::Map::new();
        if let Some(s) = self.stream {
            m.insert("stream".into(), s.name().into());
        }
        Some(m)
    }
}
