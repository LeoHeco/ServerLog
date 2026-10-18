//! Text pieces shared by the layouts: quoting, field lists, logfmt and the pretty layout.

use std::fmt::{self, Write};

use super::{Theme, colour::paint, time::Clock};
use crate::{Colour, Record, Value, compat};

/// Writes `v` as a JSON string.
///
/// # Errors
/// Fails only when `w` does.
pub fn quote(w: &mut (impl Write + ?Sized), v: impl fmt::Display) -> fmt::Result {
    w.write_char('"')?;
    write!(Escape(w), "{v}")?;
    w.write_char('"')
}

pub(crate) struct Escape<'a, W: ?Sized>(pub(crate) &'a mut W);

impl<W: Write + ?Sized> Write for Escape<'_, W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut from = 0;
        for (i, c) in s.char_indices() {
            let esc = match c {
                '"' => "\\\"",
                '\\' => "\\\\",
                '\n' => "\\n",
                '\r' => "\\r",
                '\t' => "\\t",
                c if c < ' ' => "",
                _ => continue,
            };
            self.0.write_str(&s[from..i])?;
            if esc.is_empty() {
                write!(self.0, "\\u{:04x}", u32::from(c))?;
            } else {
                self.0.write_str(esc)?;
            }
            from = i + c.len_utf8();
        }
        self.0.write_str(&s[from..])
    }
}

/// Writes `v` bare, or quoted when it is empty or holds spaces, quotes or `=`.
pub(crate) fn logfmt(w: &mut (impl Write + ?Sized), v: impl fmt::Display) -> fmt::Result {
    let s = v.to_string();
    if !s.is_empty() && !s.contains(|c: char| c.is_whitespace() || c == '"' || c == '=') {
        return w.write_str(&s);
    }
    quote(w, s)
}

/// Calls `f` with each field, groups spread into `group.key`.
pub(crate) fn flat(rec: &Record<'_>, skip: &[&str], f: &mut dyn FnMut(&str, Value<'_>)) {
    fn walk(prefix: &str, v: Value<'_>, f: &mut dyn FnMut(&str, Value<'_>)) {
        match v {
            Value::Map(m) => m.each(&mut |k, v| walk(&format!("{prefix}.{k}"), v, f)),
            v => f(prefix, v),
        }
    }
    for (k, v) in rec.fields().filter(|(k, _)| !skip.contains(k)) {
        walk(k, v, f);
    }
}

/// ` key=value` per field.
pub(crate) fn fields(
    w: &mut (impl Write + ?Sized),
    rec: &Record<'_>,
    quoted: bool,
    skip: &[&str],
) -> fmt::Result {
    let mut r = Ok(());
    flat(rec, skip, &mut |k, v| {
        if r.is_ok() {
            r = write!(w, " {k}=").and_then(|()| {
                if quoted {
                    logfmt(w, v)
                } else {
                    write!(w, "{v}")
                }
            });
        }
    });
    r
}

/// `time=… level=… target=… msg=… key=value…`
pub(crate) fn render_logfmt(out: &mut String, rec: &Record<'_>) -> fmt::Result {
    out.push_str("time=");
    Clock::Utc.write(out, rec.time())?;
    write!(out, " level={} target=", rec.level().name())?;
    logfmt(out, rec.target())?;
    out.push_str(" msg=");
    logfmt(out, rec.msg())?;
    fields(out, rec, true, &[])
}

/// A first line with time, level, target and message, then one aligned line per field.
pub(crate) fn render_pretty(
    out: &mut String,
    rec: &Record<'_>,
    theme: Option<&Theme>,
) -> fmt::Result {
    let on = theme.is_some();
    paint(out, on, Colour::Grey, |o| {
        let _ = Clock::Local.write(o, rec.time());
    });
    out.push(' ');
    let colour = theme.map_or(Colour::Plain, |t| t.of(rec.level()));
    paint(out, on, colour, |o| {
        let _ = write!(o, "{:<5}", rec.level().label(compat()));
    });
    out.push(' ');
    paint(out, on, Colour::White, |o| o.push_str(rec.target()));
    write!(out, " {}", rec.msg())?;
    let mut rows: Vec<(String, String)> = Vec::new();
    flat(rec, &[], &mut |k, v| {
        rows.push((k.to_owned(), v.to_string()));
    });
    let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (k, v) in rows {
        out.push_str("\n    ");
        paint(out, on, Colour::Cyan, |o| {
            let _ = write!(o, "{k:<width$}");
        });
        write!(out, " = {v}")?;
    }
    Ok(())
}
