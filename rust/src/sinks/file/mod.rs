//! A log file, rolled over by size or time, cleaned up by count, age or total size, and reopened when moved.

mod roll;

use std::{
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

pub use roll::Rotate;

use crate::{Entry, Error, Level, Result, Sink};
use roll::Keep;

/// Appends entries to a file.
#[must_use]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is a switch of its own in the config"
)]
pub struct File {
    path: PathBuf,
    append: bool,
    perms: Option<u32>,
    mkdir: bool,
    max_size: Option<u64>,
    rotate: Option<Rotate>,
    keep: Keep,
    gzip: bool,
    roll_name: Option<String>,
    reopen: bool,
    buffer: usize,
    flush_every: Duration,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    out: Option<BufWriter<fs::File>>,
    size: u64,
    period: Option<String>,
    began: Option<SystemTime>,
    checked: Option<Instant>,
    flushed: Option<Instant>,
    jobs: Vec<JoinHandle<()>>,
}

impl File {
    /// Appends to `path`, creating it and its directory, never rolling over.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            append: true,
            perms: None,
            mkdir: true,
            max_size: None,
            rotate: None,
            keep: Keep::default(),
            gzip: false,
            roll_name: None,
            reopen: false,
            buffer: 0,
            flush_every: Duration::from_secs(1),
            state: Mutex::default(),
        }
    }

    /// Empties the file at start-up instead of appending.
    pub const fn truncate(mut self) -> Self {
        self.append = false;
        self
    }

    /// Permission bits for a newly created file, such as `0o640`; ignored on Windows.
    pub const fn perms(mut self, mode: u32) -> Self {
        self.perms = Some(mode);
        self
    }

    /// Fails instead of creating a missing directory.
    pub const fn no_mkdir(mut self) -> Self {
        self.mkdir = false;
        self
    }

    /// Rolls over before the file would pass `bytes`.
    pub const fn max_size(mut self, bytes: u64) -> Self {
        self.max_size = Some(bytes);
        self
    }

    /// Rolls over when a new minute, hour, day or week starts.
    pub const fn rotate(mut self, every: Rotate) -> Self {
        self.rotate = Some(every);
        self
    }

    /// Keeps only the newest `n` rolled-over files.
    pub const fn keep(mut self, n: usize) -> Self {
        self.keep.count = Some(n);
        self
    }

    /// Deletes rolled-over files older than `age`.
    pub const fn max_age(mut self, age: Duration) -> Self {
        self.keep.age = Some(age);
        self
    }

    /// Deletes the oldest rolled-over files while they add up to more than `bytes`.
    pub const fn max_total(mut self, bytes: u64) -> Self {
        self.keep.total = Some(bytes);
        self
    }

    /// Compresses rolled-over files in the background.
    pub const fn gzip(mut self) -> Self {
        self.gzip = true;
        self
    }

    /// Names rolled-over files from a template with `{stem}`, `{ext}` and `{time:strftime}`.
    pub fn roll_name(mut self, template: impl Into<String>) -> Self {
        self.roll_name = Some(template.into());
        self
    }

    /// Reopens the file when something outside moves, truncates or deletes it, as logrotate does.
    pub const fn reopen(mut self) -> Self {
        self.reopen = true;
        self
    }

    /// Buffers up to `bytes` and flushes every `every`, or at once for errors.
    pub const fn buffer(mut self, bytes: usize, every: Duration) -> Self {
        self.buffer = bytes;
        self.flush_every = every;
        self
    }

    /// The path written to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn template(&self) -> &str {
        self.roll_name.as_deref().unwrap_or(roll::NAME)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn open(&self, fresh: bool) -> io::Result<fs::File> {
        let mut o = OpenOptions::new();
        o.create(true);
        if fresh && !self.append {
            o.write(true).truncate(true);
        } else {
            o.append(true);
        }
        #[cfg(unix)]
        if let Some(mode) = self.perms {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(mode);
        }
        o.open(&self.path)
    }

    fn start(&self, st: &mut State, fresh: bool) -> io::Result<()> {
        let f = self.open(fresh)?;
        let meta = f.metadata()?;
        st.size = meta.len();
        st.began = Some(meta.modified().unwrap_or_else(|_| SystemTime::now()));
        st.period = self
            .rotate
            .map(|r| r.period(st.began.unwrap_or_else(SystemTime::now)));
        st.out = Some(BufWriter::with_capacity(self.buffer, f));
        Ok(())
    }

    fn moved(&self, st: &State) -> bool {
        match fs::metadata(&self.path) {
            Err(_) => true,
            Ok(m) => m.len() < st.size,
        }
    }

    fn roll(&self, st: &mut State, now: SystemTime) -> io::Result<()> {
        if let Some(mut out) = st.out.take() {
            out.flush()?;
        }
        let to = roll::name(&self.path, self.template(), st.began.unwrap_or(now));
        fs::rename(&self.path, &to)?;
        self.start(st, false)?;
        st.began = Some(now);
        st.period = self.rotate.map(|r| r.period(now));
        let (gzip, keep, path, template) = (
            self.gzip,
            self.keep,
            self.path.clone(),
            self.template().to_owned(),
        );
        for j in st.jobs.drain(..) {
            let _ = j.join();
        }
        if let Ok(j) = thread::Builder::new()
            .name("serlog-roll".into())
            .spawn(move || {
                if gzip {
                    let _ = roll::compress(&to);
                }
                roll::prune(&path, &template, keep);
            })
        {
            st.jobs.push(j);
        }
        Ok(())
    }

    fn put(&self, text: &str, time: SystemTime, urgent: bool) -> Result {
        let mut st = self.state();
        let now = Instant::now();
        if self.reopen
            && st.out.is_some()
            && st
                .checked
                .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1))
        {
            st.checked = Some(now);
            if self.moved(&st) {
                st.out = None;
            }
        }
        if st.out.is_none() {
            self.start(&mut st, false)?;
        }
        let len = text.len() as u64;
        let full = self
            .max_size
            .is_some_and(|max| st.size > 0 && st.size + len > max);
        let turned = self
            .rotate
            .is_some_and(|r| st.period.as_deref() != Some(r.period(time).as_str()));
        if full || turned {
            self.roll(&mut st, time)?;
        }
        let out = st
            .out
            .as_mut()
            .ok_or_else(|| Error::other("file not open"))?;
        out.write_all(text.as_bytes())?;
        if self.buffer > 0
            && (urgent
                || st
                    .flushed
                    .is_none_or(|t| now.duration_since(t) >= self.flush_every))
        {
            if let Some(out) = st.out.as_mut() {
                out.flush()?;
            }
            st.flushed = Some(now);
        }
        st.size += len;
        Ok(())
    }
}

impl Sink for File {
    fn emit(&self, entry: &Entry<'_>) -> Result {
        let rec = entry.record();
        let line = entry.line();
        let mut text = String::with_capacity(line.len() + 1);
        text.push_str(line);
        text.push('\n');
        self.put(&text, rec.time(), rec.level() >= Level::ERROR)
    }

    fn emit_batch(&self, entries: &[Entry<'_>]) -> Result {
        let Some(last) = entries.last() else {
            return Ok(());
        };
        if self.max_size.is_some() || self.rotate.is_some() {
            return entries.iter().try_for_each(|e| self.emit(e));
        }
        let mut text = String::new();
        for e in entries {
            text.push_str(e.line());
            text.push('\n');
        }
        let urgent = entries.iter().any(|e| e.record().level() >= Level::ERROR);
        self.put(&text, last.record().time(), urgent)
    }

    fn flush(&self) -> Result {
        let jobs = {
            let mut st = self.state();
            if let Some(out) = st.out.as_mut() {
                out.flush()?;
            }
            std::mem::take(&mut st.jobs)
        };
        for j in jobs {
            let _ = j.join();
        }
        Ok(())
    }

    fn ready(&self) -> Result {
        if self.path.is_dir() {
            return Err(Error::Config(format!(
                "{} is a directory",
                self.path.display()
            )));
        }
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            if self.mkdir {
                fs::create_dir_all(dir)?;
            } else if !dir.is_dir() {
                return Err(Error::Config(format!("{} does not exist", dir.display())));
            }
        }
        let mut st = self.state();
        self.start(&mut st, true)?;
        Ok(())
    }

    fn claim(&self) -> Option<String> {
        let p = fs::canonicalize(&self.path)
            .or_else(|e| {
                let name = self.path.file_name().ok_or(e)?;
                let dir = self.path.parent().filter(|d| !d.as_os_str().is_empty());
                fs::canonicalize(dir.unwrap_or(Path::new("."))).map(|d| d.join(name))
            })
            .or_else(|_| std::path::absolute(&self.path))
            .unwrap_or_else(|_| self.path.clone());
        let s = p.to_string_lossy().into_owned();
        Some(format!(
            "file {}",
            if cfg!(windows) { s.to_lowercase() } else { s }
        ))
    }

    fn kind(&self) -> &'static str {
        "file"
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        use crate::config::units::{show, show_size};
        let mut m = serde_json::Map::new();
        m.insert(
            "path".into(),
            self.path.to_string_lossy().into_owned().into(),
        );
        if !self.append {
            m.insert("append".into(), false.into());
        }
        if let Some(p) = self.perms {
            m.insert("perms".into(), format!("{p:04o}").into());
        }
        if !self.mkdir {
            m.insert("mkdir".into(), false.into());
        }
        if let Some(n) = self.max_size {
            m.insert("max_size".into(), show_size(n).into());
        }
        if let Some(r) = self.rotate {
            m.insert("rotate".into(), r.name().into());
        }
        if let Some(n) = self.keep.count {
            m.insert("keep".into(), n.into());
        }
        if let Some(a) = self.keep.age {
            m.insert("max_age".into(), show(a).into());
        }
        if let Some(n) = self.keep.total {
            m.insert("max_total".into(), show_size(n).into());
        }
        if self.gzip {
            m.insert("gzip".into(), true.into());
        }
        if let Some(t) = &self.roll_name {
            m.insert("roll_name".into(), t.clone().into());
        }
        if self.reopen {
            m.insert("reopen".into(), true.into());
        }
        if self.buffer > 0 {
            m.insert("buffer".into(), show_size(self.buffer as u64).into());
            m.insert("flush_every".into(), show(self.flush_every).into());
        }
        Some(m)
    }
}
