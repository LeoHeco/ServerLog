//! One log event as every sink sees it, and an owned copy that can wait in a queue or be edited.

use std::{
    fmt::{self, Display},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::SystemTime,
};

use crate::{Level, ToValue, Val, Value};

/// A named value attached to a record.
pub type Field<'a> = (&'a str, Value<'a>);

static SEQ: AtomicU64 = AtomicU64::new(0);

fn next() -> u64 {
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Where in the source a record was made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caller<'a> {
    /// Module path.
    pub module: Option<&'a str>,
    /// Function.
    pub func: Option<&'a str>,
    /// Source file.
    pub file: Option<&'a str>,
    /// Line.
    pub line: Option<u32>,
}

/// A log event, borrowed for the duration of one dispatch.
#[derive(Clone, Copy)]
pub struct Record<'a> {
    level: Level,
    target: &'a str,
    msg: Msg<'a>,
    fields: Fields<'a>,
    tags: Tags<'a>,
    caller: Caller<'a>,
    thread: Option<&'a str>,
    spans: Option<&'a str>,
    time: SystemTime,
    seq: u64,
}

/// A record's message, formatted when shown.
#[derive(Clone, Copy)]
pub struct Msg<'a>(Text<'a>);

#[derive(Clone, Copy)]
enum Text<'a> {
    Args(fmt::Arguments<'a>),
    Str(&'a str),
}

#[derive(Clone, Copy)]
enum Fields<'a> {
    Refs(&'a [Field<'a>]),
    Owned(&'a [(String, Val)]),
}

#[derive(Clone, Copy)]
enum Tags<'a> {
    Refs(&'a [&'a str]),
    Owned(&'a [String]),
}

impl<'a> Record<'a> {
    /// A record stamped with the current time.
    #[must_use]
    pub fn new(level: Level, target: &'a str, args: fmt::Arguments<'a>) -> Self {
        Self {
            level,
            target,
            msg: Msg(Text::Args(args)),
            fields: Fields::Refs(&[]),
            tags: Tags::Refs(&[]),
            caller: Caller::default(),
            thread: None,
            spans: None,
            time: SystemTime::now(),
            seq: next(),
        }
    }

    /// Sets the source file and line.
    #[must_use]
    pub const fn at(mut self, file: &'a str, line: u32) -> Self {
        self.caller.file = Some(file);
        self.caller.line = Some(line);
        self
    }

    /// Sets the source file and line, either of which may be unknown.
    #[must_use]
    pub const fn located(mut self, file: Option<&'a str>, line: Option<u32>) -> Self {
        self.caller.file = file;
        self.caller.line = line;
        self
    }

    /// Sets everything known about the caller.
    #[must_use]
    pub const fn caller(mut self, caller: Caller<'a>) -> Self {
        self.caller = caller;
        self
    }

    /// Sets the fields.
    #[must_use]
    pub const fn with(mut self, fields: &'a [Field<'a>]) -> Self {
        self.fields = Fields::Refs(fields);
        self
    }

    /// Sets the tags.
    #[must_use]
    pub const fn tagged(mut self, tags: &'a [&'a str]) -> Self {
        self.tags = Tags::Refs(tags);
        self
    }

    /// Sets the span chain, written the `tracing-subscriber` way: `req{id=1}:db`.
    #[must_use]
    pub const fn spans(mut self, spans: &'a str) -> Self {
        self.spans = Some(spans);
        self
    }

    /// Overrides the time, for replaying or importing records.
    #[must_use]
    pub const fn stamped(mut self, time: SystemTime) -> Self {
        self.time = time;
        self
    }

    /// Severity.
    #[must_use]
    pub const fn level(&self) -> Level {
        self.level
    }

    /// Where it came from, usually a module path.
    #[must_use]
    pub const fn target(&self) -> &'a str {
        self.target
    }

    /// The message.
    #[must_use]
    pub const fn msg(&self) -> Msg<'a> {
        self.msg
    }

    /// Where in the source it was made.
    #[must_use]
    pub const fn site(&self) -> Caller<'a> {
        self.caller
    }

    /// Source file.
    #[must_use]
    pub const fn file(&self) -> Option<&'a str> {
        self.caller.file
    }

    /// Source line.
    #[must_use]
    pub const fn line(&self) -> Option<u32> {
        self.caller.line
    }

    /// The thread that made it, when it was made on another thread than the one showing it.
    #[must_use]
    pub const fn thread(&self) -> Option<&'a str> {
        self.thread
    }

    /// The span chain, if any.
    #[must_use]
    pub const fn span_chain(&self) -> Option<&'a str> {
        self.spans
    }

    /// When it was made.
    #[must_use]
    pub const fn time(&self) -> SystemTime {
        self.time
    }

    /// Its number in this process, counting from 0.
    #[must_use]
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    /// Fields, in order.
    #[must_use]
    pub fn fields(&self) -> impl ExactSizeIterator<Item = Field<'a>> + 'a {
        let (refs, owned) = match self.fields {
            Fields::Refs(r) => (r, &[][..]),
            Fields::Owned(o) => (&[][..], o),
        };
        Pairs { refs, owned }
    }

    /// The last field called `key`.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<Value<'a>> {
        self.fields()
            .filter(|(k, _)| *k == key)
            .last()
            .map(|(_, v)| v)
    }

    /// Tags, in order.
    pub fn tags(&self) -> impl Iterator<Item = &'a str> + 'a {
        let (refs, owned) = match self.tags {
            Tags::Refs(r) => (r, &[][..]),
            Tags::Owned(o) => (&[][..], o),
        };
        refs.iter().copied().chain(owned.iter().map(String::as_str))
    }

    /// Copies everything out, so the record can outlive this call, cross threads or be edited.
    #[must_use]
    pub fn to_buf(&self) -> RecordBuf {
        let own = |s: Option<&str>| s.map(str::to_owned);
        RecordBuf {
            level: self.level,
            target: self.target.to_owned(),
            msg: self.msg.to_string(),
            fields: self
                .fields()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            tags: self.tags().map(str::to_owned).collect(),
            module: own(self.caller.module),
            func: own(self.caller.func),
            file: own(self.caller.file),
            line: self.caller.line,
            thread: own(self.thread).or_else(|| thread::current().name().map(str::to_owned)),
            spans: own(self.spans),
            time: self.time,
            seq: self.seq,
        }
    }
}

struct Pairs<'a> {
    refs: &'a [Field<'a>],
    owned: &'a [(String, Val)],
}

impl<'a> Iterator for Pairs<'a> {
    type Item = Field<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if let [first, rest @ ..] = self.refs {
            self.refs = rest;
            return Some(*first);
        }
        let [(k, v), rest @ ..] = self.owned else {
            return None;
        };
        self.owned = rest;
        Some((k.as_str(), v.to_value()))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.refs.len() + self.owned.len();
        (n, Some(n))
    }
}

impl ExactSizeIterator for Pairs<'_> {}

impl Msg<'_> {
    /// The text, when it needs no formatting.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self.0 {
            Text::Args(a) => a.as_str(),
            Text::Str(s) => Some(s),
        }
    }
}

impl Display for Msg<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Text::Args(a) => f.write_fmt(a),
            Text::Str(s) => f.write_str(s),
        }
    }
}

impl fmt::Debug for Msg<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string(), f)
    }
}

impl fmt::Debug for Record<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Record")
            .field("level", &self.level)
            .field("target", &self.target)
            .field("msg", &self.msg)
            .field("fields", &self.fields().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// An owned record: what a queue holds, what an alarm sends, and what processors edit.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordBuf {
    level: Level,
    target: String,
    msg: String,
    fields: Vec<(String, Val)>,
    tags: Vec<String>,
    module: Option<String>,
    func: Option<String>,
    file: Option<String>,
    line: Option<u32>,
    thread: Option<String>,
    spans: Option<String>,
    time: SystemTime,
    seq: u64,
}

impl RecordBuf {
    /// A record made now on this thread.
    #[must_use]
    pub fn new(level: Level, target: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            level,
            target: target.into(),
            msg: msg.into(),
            fields: Vec::new(),
            tags: Vec::new(),
            module: None,
            func: None,
            file: None,
            line: None,
            thread: thread::current().name().map(str::to_owned),
            spans: None,
            time: SystemTime::now(),
            seq: next(),
        }
    }

    /// Adds a field.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl ToValue) -> Self {
        self.fields.push((key.into(), value.to_value().to_owned()));
        self
    }

    /// Adds a tag.
    #[must_use]
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Severity.
    #[must_use]
    pub const fn level(&self) -> Level {
        self.level
    }

    /// Changes the severity.
    pub const fn set_level(&mut self, level: Level) {
        self.level = level;
    }

    /// The message.
    #[must_use]
    pub fn msg(&self) -> &str {
        &self.msg
    }

    /// Replaces the message.
    pub fn set_msg(&mut self, msg: impl Into<String>) {
        self.msg = msg.into();
    }

    /// The fields.
    #[must_use]
    pub fn fields(&self) -> &[(String, Val)] {
        &self.fields
    }

    /// The last field called `key`.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&Val> {
        self.fields
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// The tags.
    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// The fields, for editing in place.
    pub const fn fields_mut(&mut self) -> &mut Vec<(String, Val)> {
        &mut self.fields
    }

    /// Sets `key`, replacing an earlier field of that name.
    pub fn set(&mut self, key: &str, value: impl ToValue) {
        let v = value.to_value().to_owned();
        match self.fields.iter_mut().find(|(k, _)| k == key) {
            Some((_, old)) => *old = v,
            None => self.fields.push((key.to_owned(), v)),
        }
    }

    /// The tags, for editing in place.
    pub const fn tags_mut(&mut self) -> &mut Vec<String> {
        &mut self.tags
    }

    /// Borrows it as a [`Record`].
    #[must_use]
    pub fn as_record(&self) -> Record<'_> {
        Record {
            level: self.level,
            target: &self.target,
            msg: Msg(Text::Str(&self.msg)),
            fields: Fields::Owned(&self.fields),
            tags: Tags::Owned(&self.tags),
            caller: Caller {
                module: self.module.as_deref(),
                func: self.func.as_deref(),
                file: self.file.as_deref(),
                line: self.line,
            },
            thread: self.thread.as_deref(),
            spans: self.spans.as_deref(),
            time: self.time,
            seq: self.seq,
        }
    }
}

impl From<&Record<'_>> for RecordBuf {
    fn from(rec: &Record<'_>) -> Self {
        rec.to_buf()
    }
}
