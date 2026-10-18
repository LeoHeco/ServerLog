//! Rolled-over files: their names, compression and clean-up.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use flate2::{Compression, write::GzEncoder};

use crate::layout::time::local;

/// When files roll over by time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rotate {
    /// On the minute.
    Minute,
    /// On the hour.
    Hour,
    /// At local midnight.
    Day,
    /// At the start of an ISO week.
    Week,
}

impl Rotate {
    /// The name used in config files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
        }
    }

    /// A key that changes exactly when a new period starts.
    pub(super) fn period(self, t: SystemTime) -> String {
        let fmt = match self {
            Self::Minute => "%Y%m%d%H%M",
            Self::Hour => "%Y%m%d%H",
            Self::Day => "%Y%m%d",
            Self::Week => "%G%V",
        };
        local(t).strftime(fmt).to_string()
    }
}

impl std::str::FromStr for Rotate {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        [Self::Minute, Self::Hour, Self::Day, Self::Week]
            .into_iter()
            .find(|r| r.name() == s)
            .ok_or_else(|| format!("`rotate` takes minute, hour, day or week, not `{s}`"))
    }
}

/// The default name of a rolled file.
pub const NAME: &str = "{stem}.{time:%Y%m%d-%H%M%S}.{ext}";

pub(super) fn parts(path: &Path) -> (String, String) {
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    (stem, ext)
}

/// Fills `template` for `path` begun at `began`, adding `-1`, `-2`… before the extension until the name is free.
pub(super) fn name(path: &Path, template: &str, began: SystemTime) -> PathBuf {
    let (stem, ext) = parts(path);
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let Some(end) = rest[i..].find('}') else {
            break;
        };
        let hole = &rest[i + 1..i + end];
        match hole.split_once(':') {
            Some(("time", f)) => out.push_str(&local(began).strftime(f).to_string()),
            _ if hole == "stem" => out.push_str(&stem),
            _ if hole == "ext" => out.push_str(&ext),
            _ => out.push_str(&rest[i..=i + end]),
        }
        rest = &rest[i + end + 1..];
    }
    out.push_str(rest);
    let out = out.trim_end_matches('.').to_owned();
    let free = |p: &Path| !p.exists() && !gz(p).exists();
    let first = path.with_file_name(&out);
    if free(&first) {
        return first;
    }
    let (base, tail) = out
        .rsplit_once('.')
        .map_or((out.as_str(), String::new()), |(b, t)| (b, format!(".{t}")));
    (1..u32::MAX)
        .map(|n| path.with_file_name(format!("{base}-{n}{tail}")))
        .find(|p| free(p))
        .unwrap_or(first)
}

pub(super) fn gz(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".gz");
    s.into()
}

pub(super) fn compress(path: &Path) -> io::Result<()> {
    let mut enc = GzEncoder::new(fs::File::create(gz(path))?, Compression::default());
    io::copy(&mut fs::File::open(path)?, &mut enc)?;
    let out = enc.finish()?;
    if let Ok(t) = fs::metadata(path).and_then(|m| m.modified()) {
        let _ = out.set_modified(t);
    }
    fs::remove_file(path)
}

/// The names a template makes: its time becomes `*`, and a `-N` may come before the extension.
struct Shape {
    plain: String,
    numbered: String,
}

fn shape(path: &Path, template: &str) -> Shape {
    let (stem, ext) = parts(path);
    let mut plain = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        plain.push_str(&rest[..i]);
        let end = rest[i..].find('}').map_or(rest.len(), |e| i + e);
        plain.push_str(match rest.get(i + 1..end).unwrap_or_default() {
            "stem" => &stem,
            "ext" => &ext,
            _ => "*",
        });
        rest = rest.get(end + 1..).unwrap_or_default();
    }
    plain.push_str(rest);
    let plain = plain.trim_end_matches('.').to_owned();
    let numbered = match plain.rsplit_once('.') {
        Some((base, tail)) => format!("{base}*.{tail}"),
        None => format!("{plain}*"),
    };
    Shape { plain, numbered }
}

impl Shape {
    fn matches(&self, name: &str) -> bool {
        let name = name.strip_suffix(".gz").unwrap_or(name);
        glob(self.plain.as_bytes(), name.as_bytes())
            || glob(self.numbered.as_bytes(), name.as_bytes())
    }
}

fn glob(pat: &[u8], s: &[u8]) -> bool {
    match pat.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|i| glob(rest, &s[i..])),
        Some((c, rest)) => s
            .split_first()
            .is_some_and(|(d, tail)| c == d && glob(rest, tail)),
    }
}

/// What clean-up keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Keep {
    pub(super) count: Option<usize>,
    pub(super) age: Option<Duration>,
    pub(super) total: Option<u64>,
}

/// Deletes rolled siblings of `path` beyond what `keep` allows, oldest first.
pub(super) fn prune(path: &Path, template: &str, keep: Keep) {
    if keep == Keep::default() {
        return;
    }
    let shape = shape(path, template);
    let live = path.file_name().unwrap_or_default().to_owned();
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut old: Vec<(SystemTime, u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name();
            n != live && shape.matches(&n.to_string_lossy())
        })
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    old.sort_by(|a, b| (a.0, &a.2).cmp(&(b.0, &b.2)));
    let now = SystemTime::now();
    let mut total: u64 = old.iter().map(|(_, n, _)| n).sum();
    let mut left = old.len();
    for (t, n, p) in &old {
        let aged = keep
            .age
            .is_some_and(|a| now.duration_since(*t).unwrap_or_default() > a);
        let many = keep.count.is_some_and(|c| left > c);
        let big = keep.total.is_some_and(|max| total > max);
        if !(aged || many || big) {
            continue;
        }
        if fs::remove_file(p).is_ok() {
            left -= 1;
            total -= n;
        }
    }
}
