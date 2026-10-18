"""The hub: global filter, context, global processors, the slot table and dispatch.

Like most loggers it starts at `info` and prints to standard output until it is configured.
"""

import threading
import time
from collections.abc import Iterable, Mapping
from typing import TYPE_CHECKING

from . import alarms, guard
from .errors import Claimed, NameTaken, NotReady
from .filter import Filter, as_filter
from .level import Level, Levelish
from .processors import Redact, Step, run
from .record import Record
from .scope import bound
from .slot import Slot, Status, as_slot
from .slot.live import Live

if TYPE_CHECKING:
    from .sink import Sink

__state = {
    "filter": Filter("info"),
    "floor": 30,
    "process": (),
    "table": (),
    "configured": False,
    "fallback": None,
    "stderr": None,
}
__edit = threading.Lock()


def enabled(level: int) -> bool:
    """Whether a record at `level` could pass the filter for some target; the calls check this first."""
    floor = __state["floor"]
    return floor is not None and level >= floor


def filter() -> Filter:
    """The global filter in force."""
    return __state["filter"]  # type: ignore[return-value]


def set_filter(f: Filter | str | Levelish) -> None:
    """Replaces the global filter; takes effect at once."""
    f = as_filter(f if not isinstance(f, Level) else int(f))
    __state["filter"] = f
    __state["floor"] = f.floor


def set_level(level: Levelish) -> None:
    """Lets through `level` and above for every target."""
    set_filter(Filter(int(Level(level))))


def set_process(steps: Iterable[Step]) -> None:
    """Replaces the global processor chain."""
    __state["process"] = tuple(steps)


def process(step: Step) -> None:
    """Adds a step to the end of the global processor chain."""
    __state["process"] = (*__state["process"], step)  # type: ignore[misc]


def redact(*keys: str) -> None:
    """Masks fields with these names, wherever they appear."""
    process(Redact(keys))


def chain() -> tuple[Step, ...]:
    """The global processor chain."""
    return __state["process"]  # type: ignore[return-value]


def table() -> tuple[Live, ...]:
    """The registered slots."""
    return __state["table"]  # type: ignore[return-value]


def add(sink: Sink | Slot | Mapping[str, object], /, **keys: object) -> str:
    """Registers a sink with its slot settings at run time; returns the name it got."""
    slot = as_slot(sink) if not keys else Slot(sink, **keys)  # type: ignore[arg-type]
    with __edit:
        live = prepare([slot], table(), {})
        __state["table"] = (*table(), *live)
    __state["configured"] = True
    return live[0].name


def remove(name: str) -> bool:
    """Takes a slot out, writes what its queue holds and closes its sink. `False` when no slot has that name."""
    with __edit:
        before = table()
        gone = [x for x in before if x.name == name]
        __state["table"] = tuple(x for x in before if x.name != name)
    for x in gone:
        x.retire()
    return bool(gone)


def status() -> list[Status]:
    """Every slot's state."""
    return [x.core.status() for x in table()]


def flush() -> None:
    """Waits up to five seconds for every queue and the alarm route to drain."""
    deadline = time.monotonic() + 5
    flush_slots(5)
    alarms.settle(max(deadline - time.monotonic(), 0))


def flush_slots(within: float) -> None:
    """Waits up to `within` seconds for every slot."""
    deadline = time.monotonic() + within
    for x in table():
        x.flush(deadline)


def emit(rec: Record) -> None:
    """Sends `rec` through the filter, the context and the global processors, then to every slot on the log route."""
    if guard.suppress():
        return
    f: Filter = __state["filter"]  # type: ignore[assignment]
    if not f.allows(rec.level, rec.target) or (f.has_pattern and not f.admits(rec.msg)):
        return
    if extra := bound():
        rec = rec.replace(fields=extra + rec.fields)
    steps = chain()
    if steps:
        with guard.inside():
            out = run(steps, rec)
        if out is None:
            return
        rec = out
    dispatch(rec)


def dispatch(rec: Record) -> None:
    """Hands `rec` to every slot on the log route."""
    cache: dict = {}
    if not __state["configured"]:
        fallback().emit(rec, cache)
        return
    for x in table():
        if not x.alarm_only:
            x.emit(rec, cache)


def fallback() -> Live:
    """The console used before any configuration."""
    if __state["fallback"] is None:
        from .sinks import Console

        __state["fallback"] = Live(Slot(Console()), "console", {})
    return __state["fallback"]  # type: ignore[return-value]


def stderr() -> Live:
    """The reserved console on standard error, where the default alarm goes."""
    if __state["stderr"] is None:
        from .sinks import Console

        __state["stderr"] = Live(Slot(Console(stream="stderr"), on_fail="ignore"), "stderr", {})
    return __state["stderr"]  # type: ignore[return-value]


def prepare(slots: Iterable[Slot], existing: Iterable[Live], named: dict[str, alarms.Alarm]) -> list[Live]:
    """Names, checks and wraps new slots against those already registered; nothing is kept on error."""
    existing = tuple(existing)
    names = {x.name for x in existing} | {"*"}
    claims = {x.claim for x in existing if x.claim}
    counts: dict[str, int] = {}
    for x in existing:
        counts[x.core.kind] = counts.get(x.core.kind, 0) + 1
    out: list[Live] = []
    for slot in slots:
        kind = slot.sink.kind
        name = slot.name
        while name is None:
            counts[kind] = counts.get(kind, 0) + 1
            if (candidate := f"{kind}-{counts[kind]}") not in names:
                name = candidate
        if name in names:
            raise NameTaken(f"sink name `{name}` is taken or reserved")
        names.add(name)
        if (c := slot.sink.claim()) is not None:
            if c in claims:
                raise Claimed(f"{c} is already used by another sink")
            claims.add(c)
        try:
            slot.sink.ready()
        except Exception as e:
            raise NotReady(name, e) from e
        out.append(Live(slot, name, named))
    return out


def known(extra: Iterable[Live]) -> set[str]:
    """Every name an alarm may route to: registered slots, `stderr` and `*`."""
    return {x.name for x in (*table(), *extra)} | {"stderr", "*"}


def install(live: Iterable[Live]) -> None:
    """Puts prepared slots in the table."""
    with __edit:
        __state["table"] = (*table(), *live)
    __state["configured"] = True


def replace(live: Iterable[Live]) -> None:
    """Swaps the whole table, for reloading a config."""
    with __edit:
        old = table()
        __state["table"] = tuple(live)
    __state["configured"] = True
    for x in old:
        x.retire()


def forget() -> None:
    """Drops every background task after a fork; they are made again on first use."""
    for x in table():
        if x.lane:
            x.lane.forget()
    alarms.task.forget()


def find(name: str) -> Live | None:
    """The slot called `name`, the reserved `stderr` included."""
    return next((x for x in table() if x.name == name), None) or (stderr() if name == "stderr" else None)


class Board:
    """The registered slots, as the alarm walk sees them."""

    def logs(self) -> list[str]:
        """Names on the log route."""
        return [x.name for x in table() if not x.alarm_only]

    def resting(self, name: str) -> bool:
        """Whether the slot is resting."""
        x = find(name)
        return x is not None and x.core.resting()

    def deliver(self, name: str, rec: Record) -> None:
        """Writes `rec` to the slot straight away."""
        x = find(name)
        if x is None:
            raise LookupError(f"no sink called `{name}`")
        x.core.deliver(rec)


BOARD = Board()
