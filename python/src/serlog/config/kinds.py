"""The four kinds of extension a config file can name: sinks, processors, layouts and alarms."""

from collections.abc import Callable

from ..alarms import Alarm, Tiers
from ..errors import KindError
from ..layouts import Layout
from ..processors import Dedupe, Exclude, Fields, Limit, Match, Redact, Sample, Sanitise, Step
from ..sink import Sink
from ..sinks import Console, File, Loki, Mail, Memory

__sinks: dict[str, Callable[..., Sink]] = {"console": Console, "file": File, "loki": Loki, "mail": Mail, "memory": Memory}
__steps: dict[str, Callable[..., Step]] = {
    "redact": Redact,
    "sanitise": Sanitise,
    "fields": Fields,
    "match": Match,
    "exclude": Exclude,
    "sample": Sample,
    "limit": Limit,
    "dedupe": Dedupe,
}
__layouts: dict[str, Callable[[], Layout]] = {}
__alarms: dict[str, Callable[..., Alarm]] = {"tiers": Tiers}


def __register[T: Callable[..., object]](table: dict[str, Callable[..., object]], name: str) -> Callable[[T], T]:
    def put(make: T) -> T:
        table[name] = make
        return make

    return put


def kind[T: Callable[..., object]](name: str) -> Callable[[T], T]:
    """Lets config files name a kind of sink: `@serlog.kind("kafka")` on a `Sink` subclass or a factory."""
    return __register(__sinks, name)  # type: ignore[arg-type]


def processor[T: Callable[..., object]](name: str) -> Callable[[T], T]:
    """Lets config files name a processor in `process`."""
    return __register(__steps, name)  # type: ignore[arg-type]


def layout[T: Callable[..., object]](name: str) -> Callable[[T], T]:
    """Lets config files name a layout with `layout: name`."""
    return __register(__layouts, name)  # type: ignore[arg-type]


def alarm[T: Callable[..., object]](name: str) -> Callable[[T], T]:
    """Lets config files name a kind of alarm with `kind: name` under `alarms`."""
    return __register(__alarms, name)  # type: ignore[arg-type]


def __find[F](table: dict[str, F], name: str, what: str) -> F:
    try:
        return table[name]
    except KeyError:
        raise KindError(f"unknown {what} kind `{name}`") from None


def sink_maker(name: str) -> Callable[..., Sink]:
    """The factory for a sink kind."""
    return __find(__sinks, name, "sink")


def step_maker(name: str) -> Callable[..., Step]:
    """The factory for a processor kind."""
    return __find(__steps, name, "processor")


def layout_maker(name: str) -> Callable[[], Layout]:
    """The factory for a named layout."""
    return __find(__layouts, name, "layout")


def alarm_maker(name: str) -> Callable[..., Alarm]:
    """The factory for an alarm kind."""
    return __find(__alarms, name, "alarm")
