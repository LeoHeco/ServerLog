"""A modern, high-performance logging framework: any number of sinks, each configured on its own.

    import serlog

    serlog.info("synced {} files", 42, mirror="debian")
    serlog.init(sinks=[serlog.Console(), serlog.File("logs/app.log", rotate="day", keep=7, name="app")])

With no set-up at all it prints `info` and above to standard output, like most loggers.
"""

from . import config, layouts, processors
from .alarms import Alarm, Failure, Fallback, Incident, Phase, Tiers, set_alarm
from .alarms import dropped as alarms_dropped
from .bridge import Handler, capture
from .colour import Colour, Theme, When
from .compat import Compat, Stream, compat
from .config import dump, load, loads
from .config.kinds import alarm, kind, layout, processor
from .errors import Claimed, ConfigError, KindError, NameTaken, NotReady, Rejected, SerlogError, Twice
from .filter import Filter
from .hub import add, emit, enabled, filter, flush, process, redact, remove, set_filter, set_level, set_process, status
from .layouts import Json, Layout, Levels, Pattern, Schema
from .level import Level
from .logger import Lazy, Logger, catch, catch_uncaught, critical, debug, error, exception, fatal, get, info, lazy, log, trace, warn, warning
from .meta import Meta, meta
from .processors import Dedupe, Exclude, Fields, Limit, Match, Processor, Redact, Sample, Sanitise
from .queues import Full, Queue
from .record import Record
from .scope import bind
from .sink import Entry, FunctionSink, Sink, SlotKeys
from .sinks import Console, File, Loki, Mail, Memory, Rotate
from .slot import Health, Slot, Status
from .start import Settings, init

__all__ = [
    "Alarm",
    "Claimed",
    "Colour",
    "Compat",
    "ConfigError",
    "Console",
    "Dedupe",
    "Entry",
    "Exclude",
    "Failure",
    "Fallback",
    "Fields",
    "File",
    "Filter",
    "Full",
    "FunctionSink",
    "Handler",
    "Health",
    "Incident",
    "Json",
    "KindError",
    "Layout",
    "Lazy",
    "Level",
    "Levels",
    "Limit",
    "Logger",
    "Loki",
    "Mail",
    "Match",
    "Memory",
    "Meta",
    "NameTaken",
    "NotReady",
    "Pattern",
    "Phase",
    "Processor",
    "Queue",
    "Record",
    "Redact",
    "Rejected",
    "Rotate",
    "Sample",
    "Sanitise",
    "Schema",
    "SerlogError",
    "Settings",
    "Sink",
    "Slot",
    "SlotKeys",
    "Status",
    "Stream",
    "Theme",
    "Tiers",
    "Twice",
    "When",
    "add",
    "alarm",
    "alarms_dropped",
    "bind",
    "capture",
    "catch",
    "catch_uncaught",
    "compat",
    "config",
    "critical",
    "debug",
    "dump",
    "emit",
    "enabled",
    "error",
    "exception",
    "fatal",
    "filter",
    "flush",
    "get",
    "info",
    "init",
    "kind",
    "layout",
    "layouts",
    "lazy",
    "load",
    "loads",
    "log",
    "meta",
    "process",
    "processor",
    "processors",
    "redact",
    "remove",
    "set_alarm",
    "set_filter",
    "set_level",
    "set_process",
    "status",
    "trace",
    "warn",
    "warning",
]
