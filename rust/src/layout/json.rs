//! JSON lines in the shape other tools expect: serlog's own, pino, bunyan, zap, slog, structlog,
//! logstash, ECS, GELF or the OpenTelemetry log model; any key can be renamed.

use std::{
    fmt::{self, Write},
    str::FromStr,
};

use super::{
    text::{Escape, quote},
    time::{self, Clock},
};
use crate::{Compat, Level, Record, Value, meta};

/// A JSON shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Schema {
    /// serlog's own.
    #[default]
    Serlog,
    /// pino.
    Pino,
    /// bunyan.
    Bunyan,
    /// zap's production encoder.
    Zap,
    /// slog's `JSONHandler`.
    Slog,
    /// structlog's `JSONRenderer`.
    Structlog,
    /// logstash-logback-encoder.
    Logstash,
    /// Elastic Common Schema.
    Ecs,
    /// Graylog Extended Log Format.
    Gelf,
    /// The OpenTelemetry log data model.
    Otel,
}

/// How levels are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Levels {
    /// By name.
    Name,
    /// By number, 10 to 60.
    Number,
}

/// A JSON layout: a schema, renamed keys, level style and where fields go.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Json {
    schema: Schema,
    keys: Vec<(String, String)>,
    levels: Option<Levels>,
    nest: Option<String>,
}

impl Schema {
    const ALL: [Self; 10] = [
        Self::Serlog,
        Self::Pino,
        Self::Bunyan,
        Self::Zap,
        Self::Slog,
        Self::Structlog,
        Self::Logstash,
        Self::Ecs,
        Self::Gelf,
        Self::Otel,
    ];

    /// The name used in config files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Serlog => "serlog",
            Self::Pino => "pino",
            Self::Bunyan => "bunyan",
            Self::Zap => "zap",
            Self::Slog => "slog",
            Self::Structlog => "structlog",
            Self::Logstash => "logstash",
            Self::Ecs => "ecs",
            Self::Gelf => "gelf",
            Self::Otel => "otel",
        }
    }
}

impl FromStr for Schema {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|x| x.name() == s)
            .ok_or_else(|| format!("unknown JSON schema `{s}`"))
    }
}

impl FromStr for Levels {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "name" => Ok(Self::Name),
            "number" => Ok(Self::Number),
            _ => Err(format!("`levels` takes name or number, not `{s}`")),
        }
    }
}

impl Json {
    /// A layout in `schema`.
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            ..Self::default()
        }
    }

    /// Writes `key` as `to`, such as `msg` as `message`.
    pub fn rename(mut self, key: impl Into<String>, to: impl Into<String>) -> Self {
        self.keys.push((key.into(), to.into()));
        self
    }

    /// Writes levels by name or by number, whatever the schema does.
    pub const fn levels(mut self, levels: Levels) -> Self {
        self.levels = Some(levels);
        self
    }

    /// Puts the fields under `key` instead of at the top.
    pub fn nest(mut self, key: impl Into<String>) -> Self {
        self.nest = Some(key.into());
        self
    }

    /// The schema.
    #[must_use]
    pub const fn schema(&self) -> Schema {
        self.schema
    }

    /// The renamed keys.
    #[must_use]
    pub fn renamed(&self) -> &[(String, String)] {
        &self.keys
    }

    /// The level style, when set.
    #[must_use]
    pub const fn level_style(&self) -> Option<Levels> {
        self.levels
    }

    /// The key fields are nested under, when set.
    #[must_use]
    pub fn nested(&self) -> Option<&str> {
        self.nest.as_deref()
    }

    /// Writes `rec` as one JSON object.
    ///
    /// # Errors
    /// Fails only when `w` does.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per schema reads best side by side"
    )]
    pub fn write(&self, w: &mut String, rec: &Record<'_>) -> fmt::Result {
        let mut o = Obj::new(w, self);
        let ids = meta();
        let level = rec.level();
        let file_line = || match (rec.file(), rec.line()) {
            (Some(f), Some(l)) => Some(format!("{f}:{l}")),
            (Some(f), None) => Some(f.to_owned()),
            _ => None,
        };
        let name_or = |o: &mut Obj<'_>, key: &str, name: &str| match self.levels {
            Some(Levels::Number) => o.raw(key, level.value()),
            _ => o.str(key, name),
        };
        match self.schema {
            Schema::Serlog => {
                o.str("time", time::iso(rec.time()))?;
                name_or(&mut o, "level", level.name())?;
                o.str("target", rec.target())?;
                o.str("msg", rec.msg())?;
                o.opt("file", rec.file())?;
                if let Some(l) = rec.line() {
                    o.raw("line", l)?;
                }
                o.fields(rec, "error")?;
                o.ids()?;
            }
            Schema::Pino => {
                if self.levels == Some(Levels::Name) {
                    o.str("level", level.name())?;
                } else {
                    o.raw("level", level.value())?;
                }
                o.raw("time", Clock::UnixMs.text(rec.time()))?;
                o.raw("pid", std::process::id())?;
                o.str("hostname", meta::host())?;
                o.fields(rec, "err")?;
                o.str("msg", rec.msg())?;
            }
            Schema::Bunyan => {
                o.str("name", &ids.app)?;
                o.str("hostname", meta::host())?;
                o.raw("pid", std::process::id())?;
                if self.levels == Some(Levels::Name) {
                    o.str("level", level.name())?;
                } else {
                    o.raw("level", level.value())?;
                }
                o.fields(rec, "err")?;
                o.str("msg", rec.msg())?;
                o.str("time", time::iso(rec.time()))?;
                o.raw("v", 0)?;
            }
            Schema::Zap => {
                name_or(&mut o, "level", level.name())?;
                o.raw("ts", format_args!("{:.3}", time::unix_float(rec.time())))?;
                o.str("logger", rec.target())?;
                o.opt("caller", file_line().as_deref())?;
                o.str("msg", rec.msg())?;
                o.fields(rec, "error")?;
            }
            Schema::Slog => {
                o.str("time", local_nanos(rec))?;
                name_or(&mut o, "level", level.label(Compat::Slog))?;
                o.str("msg", rec.msg())?;
                o.fields(rec, "error")?;
            }
            Schema::Structlog => {
                o.str("event", rec.msg())?;
                o.fields(rec, "error")?;
                name_or(&mut o, "level", level.name())?;
                o.str("logger", rec.target())?;
                o.str("timestamp", time::iso(rec.time()))?;
            }
            Schema::Logstash => {
                o.str("@timestamp", local_millis(rec))?;
                o.str("@version", "1")?;
                o.str("message", rec.msg())?;
                o.str("logger_name", rec.target())?;
                let current = std::thread::current();
                o.opt("thread_name", rec.thread().or_else(|| current.name()))?;
                name_or(&mut o, "level", level.label(Compat::Logback))?;
                o.raw("level_value", logback_value(level))?;
                o.fields(rec, "stack_trace")?;
            }
            Schema::Ecs => {
                o.str("@timestamp", time::iso(rec.time()))?;
                name_or(&mut o, "log.level", level.name())?;
                o.str("message", rec.msg())?;
                o.str("ecs.version", "8.11.0")?;
                o.str("log.logger", rec.target())?;
                o.opt("log.origin.file.name", rec.file())?;
                if let Some(l) = rec.line() {
                    o.raw("log.origin.file.line", l)?;
                }
                o.raw("process.pid", std::process::id())?;
                o.str("host.hostname", meta::host())?;
                o.nonempty("service.name", &ids.app)?;
                o.nonempty("service.environment", &ids.env)?;
                o.nonempty("service.node.name", &ids.instance)?;
                o.fields(rec, "error")?;
            }
            Schema::Gelf => {
                o.str("version", "1.1")?;
                o.str("host", meta::host())?;
                o.str("short_message", rec.msg())?;
                o.raw("timestamp", format!("{:.3}", time::unix_float(rec.time())))?;
                o.raw("level", level.syslog())?;
                o.str("_target", rec.target())?;
                o.prefixed(rec, "_")?;
            }
            Schema::Otel => {
                o.str("timestamp", format_args!("{:.9}", time::stamp(rec.time())))?;
                o.str("severity_text", level.label(Compat::Serlog))?;
                o.raw("severity_number", otel_number(level))?;
                o.str("body", rec.msg())?;
                o.key("attributes")?;
                let mut a = Obj::nested(&mut *o.w, self);
                a.str("code.namespace", rec.target())?;
                a.opt("code.filepath", rec.file())?;
                if let Some(l) = rec.line() {
                    a.raw("code.lineno", l)?;
                }
                a.all(rec, "exception")?;
                a.close();
                o.key("resource")?;
                let mut r = Obj::nested(&mut *o.w, self);
                r.nonempty("service.name", &ids.app)?;
                r.nonempty("deployment.environment", &ids.env)?;
                r.nonempty("service.instance.id", &ids.instance)?;
                r.close();
            }
        }
        o.close();
        Ok(())
    }
}

fn local_nanos(rec: &Record<'_>) -> String {
    time::local(rec.time())
        .strftime("%Y-%m-%dT%H:%M:%S%.9f%:z")
        .to_string()
}

fn local_millis(rec: &Record<'_>) -> String {
    time::local(rec.time())
        .strftime("%Y-%m-%dT%H:%M:%S%.3f%:z")
        .to_string()
}

const fn logback_value(level: Level) -> u32 {
    match level.value() {
        0..=10 => 5000,
        11..=20 => 10000,
        21..=30 => 20000,
        31..=40 => 30000,
        _ => 40000,
    }
}

const fn otel_number(level: Level) -> u8 {
    match level.value() {
        0..=10 => 1,
        11..=20 => 5,
        21..=30 => 9,
        31..=40 => 13,
        41..=50 => 17,
        _ => 21,
    }
}

impl Clock {
    fn text(&self, t: std::time::SystemTime) -> String {
        let mut s = String::new();
        let _ = self.write(&mut s, t);
        s
    }
}

/// An object being written, which knows the layout's renamed keys.
struct Obj<'w> {
    w: &'w mut String,
    json: &'w Json,
    first: bool,
}

impl<'w> Obj<'w> {
    fn new(w: &'w mut String, json: &'w Json) -> Self {
        w.push('{');
        Self {
            w,
            json,
            first: true,
        }
    }

    fn nested(w: &'w mut String, json: &'w Json) -> Self {
        Self::new(w, json)
    }

    fn key(&mut self, k: &str) -> fmt::Result {
        if !std::mem::take(&mut self.first) {
            self.w.push(',');
        }
        let k = self
            .json
            .keys
            .iter()
            .find(|(from, _)| from == k)
            .map_or(k, |(_, to)| to.as_str());
        quote(self.w, k)?;
        self.w.push(':');
        Ok(())
    }

    fn str(&mut self, k: &str, v: impl fmt::Display) -> fmt::Result {
        self.key(k)?;
        quote(self.w, v)
    }

    fn raw(&mut self, k: &str, v: impl fmt::Display) -> fmt::Result {
        self.key(k)?;
        write!(self.w, "{v}")
    }

    fn opt(&mut self, k: &str, v: Option<&str>) -> fmt::Result {
        v.map_or(Ok(()), |v| self.str(k, v))
    }

    fn nonempty(&mut self, k: &str, v: &str) -> fmt::Result {
        if v.is_empty() { Ok(()) } else { self.str(k, v) }
    }

    fn ids(&mut self) -> fmt::Result {
        let m = meta();
        self.nonempty("app", &m.app)?;
        self.nonempty("env", &m.env)?;
        self.nonempty("instance", &m.instance)
    }

    /// The fields, nested under the layout's `nest` key when set; the `error` field under `error_key`.
    fn fields(&mut self, rec: &Record<'_>, error_key: &str) -> fmt::Result {
        if rec.fields().len() == 0 {
            return Ok(());
        }
        match self.json.nest.clone() {
            Some(n) => {
                self.key(&n)?;
                let mut inner = Obj::nested(&mut *self.w, self.json);
                inner.all(rec, error_key)?;
                inner.close();
                Ok(())
            }
            None => self.all(rec, error_key),
        }
    }

    fn all(&mut self, rec: &Record<'_>, error_key: &str) -> fmt::Result {
        for (k, v) in rec.fields() {
            let k = if k == "error" { error_key } else { k };
            self.key(k)?;
            value(self.w, v)?;
        }
        Ok(())
    }

    fn prefixed(&mut self, rec: &Record<'_>, prefix: &str) -> fmt::Result {
        for (k, v) in rec.fields() {
            if k == "id" {
                continue;
            }
            self.key(&format!("{prefix}{k}"))?;
            value(self.w, v)?;
        }
        Ok(())
    }

    fn close(self) {
        self.w.push('}');
    }
}

/// Writes one value as JSON.
///
/// # Errors
/// Fails only when `w` does.
pub fn value(w: &mut String, v: Value<'_>) -> fmt::Result {
    match v {
        Value::Null => w.write_str("null"),
        Value::Bool(b) => write!(w, "{b}"),
        Value::I64(n) => write!(w, "{n}"),
        Value::U64(n) => write!(w, "{n}"),
        Value::F64(n) if n.is_finite() => write!(w, "{n}"),
        Value::F64(n) => quote(w, n),
        Value::Str(s) => quote(w, s),
        Value::Display(d) => quote(w, d),
        Value::Debug(d) => {
            w.push('"');
            write!(Escape(w), "{d:?}")?;
            w.push('"');
            Ok(())
        }
        Value::Error(e) => {
            w.push_str("{\"message\":");
            quote(w, e)?;
            let mut cause = e.source();
            if cause.is_some() {
                w.push_str(",\"causes\":[");
                let mut first = true;
                while let Some(c) = cause {
                    if !std::mem::take(&mut first) {
                        w.push(',');
                    }
                    quote(w, c)?;
                    cause = c.source();
                }
                w.push(']');
            }
            w.push('}');
            Ok(())
        }
        Value::List(l) => {
            w.push('[');
            let mut r = Ok(());
            let mut first = true;
            l.each(&mut |v| {
                if r.is_ok() {
                    if !std::mem::take(&mut first) {
                        w.push(',');
                    }
                    r = value(w, v);
                }
            });
            w.push(']');
            r
        }
        Value::Map(m) => {
            w.push('{');
            let mut r = Ok(());
            let mut first = true;
            m.each(&mut |k, v| {
                if r.is_ok() {
                    if !std::mem::take(&mut first) {
                        w.push(',');
                    }
                    r = quote(w, k).and_then(|()| {
                        w.push(':');
                        value(w, v)
                    });
                }
            });
            w.push('}');
            r
        }
    }
}
