//! Line templates: `{time} {level:<5} {target}: {msg}{fields}`, with width, alignment and truncation on any part.

use std::{
    fmt::{self, Write},
    str::FromStr,
};

use super::{
    Theme,
    colour::paint,
    preset,
    text::{self, logfmt},
    time::{self, Clock},
};
use crate::{Colour, Compat, Record, compat, meta};

/// A line template: literal text with `{token}` or `{token:spec}` holes; `{{` and `}}` are braces.
///
/// | Token | Spec | Output |
/// |:--:|:--:|:--:|
/// | `time` | `local`, `utc`, `rfc3339`, `unix`, `unix_ms`, `unix_ns` or strftime | when |
/// | `level` | `lower` | severity |
/// | `target` | `~N` shortens leading segments to fit N | origin |
/// | `module` `func` `line` | | caller |
/// | `file` | `base` keeps the file name only | caller |
/// | `thread` `tid` `task` `pid` `host` | | where it ran |
/// | `app` `env` `instance` | | process identity |
/// | `msg` `fields` | `q` quotes the logfmt way | message and ` key=value` per field |
/// | `tags` | | tags, comma-separated |
/// | `error` | `stack` | the `error` field with its causes |
/// | `elapsed` `seq` | | milliseconds since start, record number |
/// | `spans` | | the `tracing` span chain and `: `, when there is one |
///
/// Every token but `time` may end with `<N`, `>N` or `^N` to pad to N columns, then `.M` to cut at M.
///
/// A preset names its levels the way its logger does, such as `WARNING` for `python`; other patterns
/// follow the compat profile in force.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    src: String,
    pieces: Vec<Piece>,
    shows_error: bool,
    labels: Option<Compat>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Piece {
    kind: Kind,
    fit: Fit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Lit(String),
    Time(Clock),
    Level { lower: bool },
    Target { fit: Option<usize> },
    Module,
    Func,
    File { base: bool },
    Line,
    Thread,
    Tid,
    Task,
    Pid,
    Host,
    App,
    Env,
    Instance,
    Msg { quoted: bool },
    Fields { quoted: bool },
    Tags,
    Error { stack: bool },
    Elapsed,
    Seq,
    Spans,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Fit {
    align: Option<(char, usize)>,
    max: Option<usize>,
}

/// A pattern that does not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternError(String);

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PatternError {}

impl Default for Pattern {
    fn default() -> Self {
        preset::SERLOG.parse().unwrap_or(Self {
            src: String::new(),
            pieces: Vec::new(),
            shows_error: false,
            labels: None,
        })
    }
}

impl Pattern {
    /// Just the message, as the Loki sink sends by default.
    #[must_use]
    pub fn message() -> Self {
        Self {
            src: "{msg}".into(),
            pieces: vec![Piece {
                kind: Kind::Msg { quoted: false },
                fit: Fit::default(),
            }],
            shows_error: false,
            labels: None,
        }
    }
}

impl FromStr for Pattern {
    type Err = PatternError;

    /// Parses a pattern, or a preset name such as `tracing`.
    fn from_str(src: &str) -> Result<Self, Self::Err> {
        let s = preset::named(src).unwrap_or(src);
        let bad = |m: String| PatternError(format!("pattern `{s}`: {m}"));
        let mut pieces = Vec::new();
        let mut lit = String::new();
        let mut chars = s.chars().peekable();
        let flush = |lit: &mut String, pieces: &mut Vec<Piece>| {
            if !lit.is_empty() {
                pieces.push(Piece {
                    kind: Kind::Lit(std::mem::take(lit)),
                    fit: Fit::default(),
                });
            }
        };
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    lit.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    lit.push('}');
                }
                '{' => {
                    let hole: String = chars.by_ref().take_while(|&c| c != '}').collect();
                    flush(&mut lit, &mut pieces);
                    let (name, spec) = hole.split_once(':').unwrap_or((&hole, ""));
                    pieces.push(piece(name.trim(), spec).map_err(bad)?);
                }
                '}' => return Err(bad("stray `}`".into())),
                c => lit.push(c),
            }
        }
        flush(&mut lit, &mut pieces);
        let shows_error = pieces.iter().any(|p| matches!(p.kind, Kind::Error { .. }));
        Ok(Self {
            src: src.to_owned(),
            pieces,
            shows_error,
            labels: preset::named(src).and_then(|_| src.parse().ok()),
        })
    }
}

fn fit(spec: &str) -> Result<Fit, String> {
    let bad = || format!("bad width `{spec}`");
    let (align, rest) = match spec.chars().next() {
        None => return Ok(Fit::default()),
        Some(a @ ('<' | '>' | '^')) => {
            let body = &spec[1..];
            let end = body.find('.').unwrap_or(body.len());
            (
                Some((a, body[..end].parse().map_err(|_| bad())?)),
                &body[end..],
            )
        }
        Some('.') => (None, spec),
        Some(_) => return Err(bad()),
    };
    let max = match rest.strip_prefix('.') {
        Some(n) => Some(n.parse().map_err(|_| bad())?),
        None if rest.is_empty() => None,
        None => return Err(bad()),
    };
    Ok(Fit { align, max })
}

fn piece(name: &str, spec: &str) -> Result<Piece, String> {
    if name == "time" {
        return Ok(Piece {
            kind: Kind::Time(Clock::parse(spec)),
            fit: Fit::default(),
        });
    }
    let option = |word: &str| spec.strip_prefix(word).map_or((false, spec), |r| (true, r));
    let (kind, rest) = match name {
        "level" => {
            let (lower, rest) = option("lower");
            (Kind::Level { lower }, rest)
        }
        "target" => match spec.strip_prefix('~') {
            Some(r) => {
                let end = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
                let n = r[..end]
                    .parse()
                    .map_err(|_| format!("bad `~` in `{spec}`"))?;
                (Kind::Target { fit: Some(n) }, &r[end..])
            }
            None => (Kind::Target { fit: None }, spec),
        },
        "file" => {
            let (base, rest) = option("base");
            (Kind::File { base }, rest)
        }
        "msg" => {
            let (quoted, rest) = option("q");
            (Kind::Msg { quoted }, rest)
        }
        "fields" => {
            let (quoted, rest) = option("q");
            (Kind::Fields { quoted }, rest)
        }
        "error" => {
            let (stack, rest) = option("stack");
            (Kind::Error { stack }, rest)
        }
        "module" => (Kind::Module, spec),
        "func" => (Kind::Func, spec),
        "line" => (Kind::Line, spec),
        "thread" => (Kind::Thread, spec),
        "tid" => (Kind::Tid, spec),
        "task" => (Kind::Task, spec),
        "pid" => (Kind::Pid, spec),
        "host" => (Kind::Host, spec),
        "app" => (Kind::App, spec),
        "env" => (Kind::Env, spec),
        "instance" => (Kind::Instance, spec),
        "tags" => (Kind::Tags, spec),
        "elapsed" => (Kind::Elapsed, spec),
        "seq" => (Kind::Seq, spec),
        "spans" => (Kind::Spans, spec),
        _ => return Err(format!("unknown token `{name}`")),
    };
    Ok(Piece {
        kind,
        fit: fit(rest)?,
    })
}

/// Shortens leading segments of `target` to their first letter until it fits in `n`.
fn shorten(target: &str, n: usize) -> String {
    if target.len() <= n {
        return target.to_owned();
    }
    let sep = if target.contains("::") { "::" } else { "." };
    let parts: Vec<&str> = target.split(sep).collect();
    let mut out: Vec<String> = parts.iter().map(|p| (*p).to_owned()).collect();
    for i in 0..parts.len().saturating_sub(1) {
        if out.join(sep).len() <= n {
            break;
        }
        out[i] = parts[i]
            .chars()
            .next()
            .map(String::from)
            .unwrap_or_default();
    }
    out.join(sep)
}

impl Pattern {
    /// The text it was parsed from, a preset name included.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.src
    }

    /// Writes `rec` through the pattern; a theme paints the level.
    ///
    /// # Errors
    /// Fails only when `w` does.
    pub fn write(&self, w: &mut String, rec: &Record<'_>, theme: Option<&Theme>) -> fmt::Result {
        let mut scratch = String::new();
        for p in &self.pieces {
            if let Kind::Lit(s) = &p.kind {
                w.push_str(s);
                continue;
            }
            let colour = match (&p.kind, theme) {
                (Kind::Level { .. }, Some(t)) => t.of(rec.level()),
                _ => Colour::Plain,
            };
            if p.fit == Fit::default() && colour == Colour::Plain {
                self.token(w, &p.kind, rec)?;
                continue;
            }
            scratch.clear();
            self.token(&mut scratch, &p.kind, rec)?;
            if let Some(max) = p.fit.max
                && let Some((cut, _)) = scratch.char_indices().nth(max)
            {
                scratch.truncate(cut);
            }
            let text = scratch.as_str();
            paint(w, theme.is_some(), colour, |o| {
                let _ = match p.fit.align {
                    Some(('<', n)) => write!(o, "{text:<n$}"),
                    Some(('>', n)) => write!(o, "{text:>n$}"),
                    Some((_, n)) => write!(o, "{text:^n$}"),
                    None => o.write_str(text),
                };
            });
        }
        Ok(())
    }

    fn token(&self, w: &mut String, kind: &Kind, rec: &Record<'_>) -> fmt::Result {
        let site = rec.site();
        match kind {
            Kind::Lit(s) => w.write_str(s),
            Kind::Time(c) => c.write(w, rec.time()),
            Kind::Level { lower: true } => w.write_str(rec.level().name()),
            Kind::Level { lower: false } => {
                w.write_str(rec.level().label(self.labels.unwrap_or_else(compat)))
            }
            Kind::Target { fit: None } => w.write_str(rec.target()),
            Kind::Target { fit: Some(n) } => w.write_str(&shorten(rec.target(), *n)),
            Kind::Module => w.write_str(site.module.unwrap_or(rec.target())),
            Kind::Func => w.write_str(site.func.unwrap_or("?")),
            Kind::File { base } => {
                let f = site.file.unwrap_or("?");
                w.write_str(if *base {
                    f.rsplit(['/', '\\']).next().unwrap_or(f)
                } else {
                    f
                })
            }
            Kind::Line => match site.line {
                Some(n) => write!(w, "{n}"),
                None => w.write_char('?'),
            },
            Kind::Thread => match rec.thread() {
                Some(t) => w.write_str(t),
                None => w.write_str(std::thread::current().name().unwrap_or("?")),
            },
            Kind::Tid => {
                let id = format!("{:?}", std::thread::current().id());
                w.write_str(id.trim_start_matches("ThreadId(").trim_end_matches(')'))
            }
            Kind::Task => w.write_char('?'),
            Kind::Pid => write!(w, "{}", std::process::id()),
            Kind::Host => w.write_str(meta::host()),
            Kind::App => w.write_str(&meta().app),
            Kind::Env => w.write_str(&meta().env),
            Kind::Instance => w.write_str(&meta().instance),
            Kind::Msg { quoted: false } => write!(w, "{}", rec.msg()),
            Kind::Msg { quoted: true } => logfmt(w, rec.msg()),
            Kind::Fields { quoted } => {
                let skip: &[&str] = if self.shows_error { &["error"] } else { &[] };
                text::fields(w, rec, *quoted, skip)
            }
            Kind::Tags => {
                for (i, t) in rec.tags().enumerate() {
                    if i > 0 {
                        w.write_char(',')?;
                    }
                    w.write_str(t)?;
                }
                Ok(())
            }
            Kind::Error { .. } => match rec.field("error") {
                Some(v) => write!(w, "{v}"),
                None => Ok(()),
            },
            Kind::Elapsed => write!(w, "{}", time::elapsed()),
            Kind::Seq => write!(w, "{}", rec.seq()),
            Kind::Spans => match rec.span_chain() {
                Some(s) if !s.is_empty() => write!(w, "{s}: "),
                _ => Ok(()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Pattern, shorten};

    #[test]
    fn parses_specs_and_shortens_targets() {
        for good in [
            "{level:lower<5}",
            "{target:~20<20}",
            "{thread:<12.12}",
            "{msg:q}",
            "{file:base}",
            "{seq:>6}",
        ] {
            assert!(good.parse::<Pattern>().is_ok(), "{good}");
        }
        for bad in [
            "{nope}",
            "{level:%5}",
            "{msg:x}",
            "a } b",
            "{level:<x}",
            "{target:~x}",
        ] {
            assert!(bad.parse::<Pattern>().is_err(), "{bad}");
        }
        assert_eq!(shorten("mirror::worker::sync", 20), "mirror::worker::sync");
        assert_eq!(shorten("mirror::worker::sync", 15), "m::worker::sync");
        assert_eq!(shorten("mirror::worker::sync", 5), "m::w::sync");
        assert_eq!(shorten("com.example.app.Main", 12), "c.e.app.Main");
    }
}
