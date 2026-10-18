//! Start-up in one expression: identity, compat profile, levels, filter, processors, sinks and alarms.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    Alarm, Colour, Compat, Error, Filter, Level, Meta, OnFail, Result, Slot, alarm, compat, hub,
    layout, meta,
    process::{Chain, Processor, Redact},
};

static STARTED: AtomicBool = AtomicBool::new(false);

/// Collects settings until [`Builder::init`] makes them global.
#[must_use]
pub struct Builder {
    meta: Meta,
    compat: Compat,
    level: Option<Level>,
    filter: Option<String>,
    filter_env: Option<String>,
    levels: Vec<(String, u8, Colour)>,
    process: Chain,
    slots: Vec<Slot>,
    alarm: Option<Arc<dyn Alarm>>,
    pub(crate) named: Vec<(String, Arc<dyn Alarm>)>,
    #[cfg(feature = "config")]
    pub(crate) watch: Option<std::time::Duration>,
}

/// Flushes every sink when dropped; keep it alive in `main`.
#[must_use = "logs still queued are lost when the guard is dropped straight away"]
#[derive(Debug)]
pub struct Guard(());

impl Drop for Guard {
    fn drop(&mut self) {
        hub::flush();
    }
}

/// Starts configuring the global logger.
pub fn builder() -> Builder {
    Builder {
        meta: Meta::detect(),
        compat: Compat::Serlog,
        level: None,
        filter: None,
        filter_env: None,
        levels: Vec::new(),
        process: Chain::default(),
        slots: Vec::new(),
        alarm: None,
        named: Vec::new(),
        #[cfg(feature = "config")]
        watch: None,
    }
}

/// Starts the way most programs want.
///
/// With the `config` feature, reads the file named by `SERLOG_CONFIG`, or the first of `serlog.yaml`,
/// `serlog.yml`, `serlog.json` and `serlog.toml` in the working directory. Otherwise prints to the
/// console at `info`; `SERLOG_FILTER` or `RUST_LOG` set the filter, `SERLOG_COMPAT` the profile and
/// `SERLOG_FORMAT` the console's format.
///
/// # Errors
/// On a bad config file or setting, or when serlog is already initialised.
pub fn init() -> Result<Guard> {
    #[cfg(feature = "config")]
    if let Some(path) = std::env::var_os("SERLOG_CONFIG")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            ["serlog.yaml", "serlog.yml", "serlog.json", "serlog.toml"]
                .into_iter()
                .map(std::path::PathBuf::from)
                .find(|p| p.exists())
        })
    {
        return crate::config::load(path);
    }
    builder().init()
}

fn console(format: Option<&str>) -> Result<Slot> {
    #[cfg(feature = "console")]
    {
        use crate::layout::{Layout, Schema};
        let slot = Slot::new(crate::sinks::Console::new());
        Ok(match format {
            None | Some("text") => slot,
            Some("json") => slot.layout(Layout::json(Schema::Serlog)),
            Some("logfmt") => slot.layout(Layout::Logfmt),
            Some("pretty") => slot.layout(Layout::Pretty),
            Some(f) => {
                return Err(Error::Config(format!(
                    "SERLOG_FORMAT `{f}` is not text, json, logfmt or pretty"
                )));
            }
        })
    }
    #[cfg(not(feature = "console"))]
    {
        let _ = format;
        Err(Error::Config(
            "no sinks given and the `console` feature is off".into(),
        ))
    }
}

/// Everything checked and built, ready to swap in.
pub(crate) struct Plan {
    meta: Meta,
    compat: Compat,
    filter: Filter,
    process: Chain,
    live: Vec<crate::slot::Live>,
    alarm: Option<Arc<dyn Alarm>>,
    named: Vec<(String, Arc<dyn Alarm>)>,
}

impl Builder {
    /// Application name.
    pub fn app(mut self, app: impl Into<String>) -> Self {
        self.meta.app = app.into();
        self
    }

    /// Deployment environment.
    pub fn env(mut self, env: impl Into<String>) -> Self {
        self.meta.env = env.into();
        self
    }

    /// Host or replica name.
    pub fn instance(mut self, instance: impl Into<String>) -> Self {
        self.meta.instance = instance.into();
        self
    }

    /// Behaves like another logger: its pattern, stream, default level and filter variable.
    pub const fn compat(mut self, compat: Compat) -> Self {
        self.compat = compat;
        self
    }

    /// Lets through `min` and above for every target; the profile's level unless set.
    pub const fn level(mut self, min: Level) -> Self {
        self.level = Some(min);
        self
    }

    /// Per-target levels such as `info,my_app=debug,hyper=off`; overrides [`level`](Self::level).
    pub fn filter(mut self, spec: impl Into<String>) -> Self {
        self.filter = Some(spec.into());
        self
    }

    /// Takes the filter from environment variable `var` when it is set, such as `RUST_LOG`.
    pub fn filter_env(mut self, var: impl Into<String>) -> Self {
        self.filter_env = Some(var.into());
        self
    }

    /// Defines a level between or beyond the built-in ones.
    pub fn define(mut self, name: impl Into<String>, value: u8, colour: Colour) -> Self {
        self.levels.push((name.into(), value, colour));
        self
    }

    /// Adds a step to the global processor chain.
    pub fn process(mut self, step: impl Processor + 'static) -> Self {
        self.process = self.process.then(step);
        self
    }

    #[cfg(feature = "config")]
    pub(crate) fn chain(mut self, chain: &Chain) -> Self {
        for s in chain.steps() {
            self.process.push(s.clone());
        }
        self
    }

    /// Masks fields with these names wherever they appear.
    pub fn redact(self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.process(Redact::keys(keys))
    }

    /// Adds a sink with its slot settings; call it again for more, of any kind.
    pub fn sink(mut self, slot: impl Into<Slot>) -> Self {
        self.slots.push(slot.into());
        self
    }

    /// Replaces the default alarm, standard error unless set.
    pub fn alarm(mut self, alarm: impl Alarm + 'static) -> Self {
        self.alarm = Some(Arc::new(alarm));
        self
    }

    #[cfg(feature = "config")]
    pub(crate) fn shared_alarm(mut self, alarm: Arc<dyn Alarm>) -> Self {
        self.alarm = Some(alarm);
        self
    }

    fn filter_now(&self, compat: Compat) -> Result<Filter> {
        let vars: Vec<&str> = self
            .filter_env
            .as_deref()
            .map_or_else(|| compat.env().to_vec(), |v| vec![v]);
        let from_env = vars
            .iter()
            .find_map(|v| std::env::var(v).ok().filter(|s| !s.trim().is_empty()));
        match from_env.or_else(|| self.filter.clone()) {
            Some(s) => s
                .parse()
                .map_err(|e| Error::Config(format!("filter `{s}`: {e}"))),
            None => Ok(Filter::level(self.level.unwrap_or_else(|| compat.level()))),
        }
    }

    /// Checks everything and builds the slots, keeping nothing on error.
    pub(crate) fn plan(mut self, alongside: &[Arc<crate::slot::Live>]) -> Result<Plan> {
        for (name, value, colour) in &self.levels {
            Level::define(name, *value, *colour)
                .map_err(|e| Error::Config(format!("level `{name}`: {e}")))?;
        }
        let compat = match std::env::var("SERLOG_COMPAT") {
            Ok(c) if !c.trim().is_empty() => c
                .trim()
                .parse()
                .map_err(|e| Error::Config(format!("SERLOG_COMPAT: {e}")))?,
            _ => self.compat,
        };
        let filter = self.filter_now(compat)?;
        if self.slots.is_empty() {
            let format = std::env::var("SERLOG_FORMAT").ok();
            self.slots
                .push(console(format.as_deref().filter(|f| !f.is_empty()))?);
        }
        let live = hub::prepare(std::mem::take(&mut self.slots), alongside)?;
        let known = hub::known(&live);
        let check = |a: &Arc<dyn Alarm>, whose: &str| -> Result {
            match a.names().into_iter().find(|n| !known.contains(n)) {
                Some(n) => Err(Error::Config(format!(
                    "{whose} routes to `{n}`, which is not a sink"
                ))),
                None => Ok(()),
            }
        };
        if let Some(a) = &self.alarm {
            check(a, "the default alarm")?;
        }
        for l in &live {
            if let OnFail::Alarm(a) = &l.core.on_fail
                && !alarm::is_default(a)
            {
                check(a, &format!("the alarm of `{}`", l.name()))?;
            }
        }
        Ok(Plan {
            meta: self.meta,
            compat,
            filter,
            process: self.process,
            live,
            alarm: self.alarm,
            named: self.named,
        })
    }

    /// Makes the settings global.
    ///
    /// # Errors
    /// [`Error::Twice`] if serlog was already initialised; otherwise the first setting or sink that failed.
    pub fn init(self) -> Result<Guard> {
        if STARTED.swap(true, Ordering::AcqRel) {
            return Err(Error::Twice);
        }
        let plan = match hub::with_table(|t| self.plan(t)) {
            Ok(p) => p,
            Err(e) => {
                STARTED.store(false, Ordering::Release);
                return Err(e);
            }
        };
        apply(plan, false);
        Ok(Guard(()))
    }
}

/// Puts a plan in force, adding to the slots or replacing them.
pub(crate) fn apply(plan: Plan, replace: bool) {
    meta::set(plan.meta);
    compat::set(plan.compat);
    hub::set_filter(plan.filter);
    hub::set_process(plan.process);
    if let Some(a) = plan.alarm {
        alarm::set_shared(a);
    }
    #[cfg(feature = "config")]
    crate::config::name_alarms(plan.named);
    #[cfg(not(feature = "config"))]
    drop(plan.named);
    layout::time::start();
    if replace {
        hub::replace(plan.live);
    } else {
        hub::install(plan.live);
    }
}
