//! A `tracing-subscriber` layer that hands every event to serlog, with its span chain.

use std::fmt::{self, Write as _};

use tracing_core::{
    Event, Interest, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record as Values},
};
use tracing_subscriber::{layer::Context, registry::LookupSpan};

use crate::{Caller, Level, Record, ToValue, Val};

/// Sends `tracing` events to serlog; event fields keep their types, spans show as `req{id=1}:db`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Layer;

const fn level(l: tracing_core::Level) -> Level {
    match l {
        tracing_core::Level::TRACE => Level::TRACE,
        tracing_core::Level::DEBUG => Level::DEBUG,
        tracing_core::Level::INFO => Level::INFO,
        tracing_core::Level::WARN => Level::WARN,
        _ => Level::ERROR,
    }
}

#[derive(Default)]
struct Typed {
    msg: String,
    rest: Vec<(&'static str, Val)>,
}

impl Visit for Typed {
    fn record_f64(&mut self, f: &Field, v: f64) {
        self.rest.push((f.name(), Val::F64(v)));
    }

    fn record_i64(&mut self, f: &Field, v: i64) {
        self.rest.push((f.name(), Val::I64(v)));
    }

    fn record_u64(&mut self, f: &Field, v: u64) {
        self.rest.push((f.name(), Val::U64(v)));
    }

    fn record_bool(&mut self, f: &Field, v: bool) {
        self.rest.push((f.name(), Val::Bool(v)));
    }

    fn record_str(&mut self, f: &Field, v: &str) {
        if f.name() == "message" {
            self.msg.push_str(v);
        } else {
            self.rest.push((f.name(), Val::Str(v.to_owned())));
        }
    }

    fn record_error(&mut self, f: &Field, v: &(dyn std::error::Error + 'static)) {
        self.rest
            .push((f.name(), Val::Error(crate::ErrorBuf::of(v))));
    }

    fn record_debug(&mut self, f: &Field, v: &dyn fmt::Debug) {
        if f.name() == "message" {
            let _ = write!(self.msg, "{v:?}");
        } else {
            self.rest.push((f.name(), Val::Str(format!("{v:?}"))));
        }
    }
}

/// A span's fields as `tracing-subscriber` prints them: `a=1 b=2`.
struct Shown(String);

impl Visit for Shown {
    fn record_str(&mut self, f: &Field, v: &str) {
        self.sep();
        let _ = write!(self.0, "{}={v}", f.name());
    }

    fn record_debug(&mut self, f: &Field, v: &dyn fmt::Debug) {
        self.sep();
        let _ = write!(self.0, "{}={v:?}", f.name());
    }
}

impl Shown {
    fn sep(&mut self) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
    }
}

impl<S> tracing_subscriber::Layer<S> for Layer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, m: &Metadata<'_>, _: Context<'_, S>) -> bool {
        m.is_span() || crate::enabled(level(*m.level()))
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut shown = Shown(String::new());
        attrs.record(&mut shown);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(shown);
        }
    }

    fn on_record(&self, id: &Id, values: &Values<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && let Some(shown) = span.extensions_mut().get_mut::<Shown>()
        {
            values.record(shown);
        }
    }

    fn on_event(&self, ev: &Event<'_>, ctx: Context<'_, S>) {
        let m = ev.metadata();
        let lvl = level(*m.level());
        if !crate::enabled(lvl) {
            return;
        }
        let mut chain = String::new();
        if let Some(scope) = ctx.event_scope(ev) {
            for span in scope.from_root() {
                if !chain.is_empty() {
                    chain.push(':');
                }
                chain.push_str(span.name());
                if let Some(Shown(f)) = span.extensions().get::<Shown>()
                    && !f.is_empty()
                {
                    let _ = write!(chain, "{{{f}}}");
                }
            }
        }
        let mut typed = Typed::default();
        ev.record(&mut typed);
        let fields: Vec<crate::Field<'_>> =
            typed.rest.iter().map(|(k, v)| (*k, v.to_value())).collect();
        let msg = &typed.msg;
        crate::emit(
            &Record::new(lvl, m.target(), format_args!("{msg}"))
                .with(&fields)
                .spans(&chain)
                .caller(Caller {
                    module: m.module_path(),
                    func: None,
                    file: m.file(),
                    line: m.line(),
                }),
        );
    }
}
