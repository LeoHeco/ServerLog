"""The layer around each sink: name, filter, processors, layout, queue, retries, health and failure policy."""

from collections.abc import Callable, Mapping
from dataclasses import dataclass
from datetime import datetime
from typing import TYPE_CHECKING, Unpack

from ..colour import Theme, When
from ..errors import ConfigError
from ..filter import Filter, as_filter
from ..layouts import Layout, as_layout, build
from ..level import Level
from ..queues import Full, Queue
from ..sink import Sink, SlotKeys
from ..units import seconds
from .breaker import Breaker, Health, Phase, Tell

if TYPE_CHECKING:
    from ..alarms import Alarm
    from ..processors import Step


@dataclass(frozen=True, slots=True)
class Status:
    """A slot's state, for monitoring."""

    name: str
    kind: str
    health: Health
    skipped: int
    dropped: int
    suppressed: int
    since: datetime
    error: str | None


class Slot:
    """A sink with the settings of the slot it goes into; keys are those of a config entry."""

    __slots__ = (
        "alarm_only",
        "colour",
        "filter",
        "given",
        "layout",
        "mode",
        "name",
        "on_fail",
        "probe",
        "process",
        "retries",
        "sink",
        "theme",
    )

    def __init__(self, sink: Sink | Callable[..., object], **keys: Unpack[SlotKeys]) -> None:
        from ..sink import FunctionSink

        self.sink: Sink = sink if isinstance(sink, Sink) else FunctionSink(sink)
        keys = {**self.sink.slot, **keys}  # type: ignore[typeddict-item]
        self.given = keys
        self.name: str | None = keys.get("name")
        level, spec = keys.get("level"), keys.get("filter")
        if level is not None and spec is not None:
            raise ConfigError("give `level` or `filter`, not both")
        self.filter: Filter | None = Filter(int(Level(level))) if level is not None else as_filter(spec) if spec is not None else None
        from ..config import make_step

        steps = keys.get("process", ())
        self.process: tuple[Step, ...] = tuple(make_step(p) if isinstance(p, str | Mapping) else p for p in steps)
        lay = keys.get("layout")
        self.layout: Layout | None = (
            as_layout(lay)
            if lay is not None and not isinstance(lay, str)
            else build(
                layout=lay,
                format=keys.get("format"),
                pattern=keys.get("pattern"),
                schema=keys.get("schema"),
                keys=keys.get("keys"),
                levels=keys.get("levels"),
                nest=keys.get("nest"),
            )
        )
        self.colour = When(keys.get("colour", "auto"))
        self.theme = Theme(keys.get("colours"))
        self.alarm_only = bool(keys.get("alarm_only", False))
        self.on_fail: str | Alarm | None = keys.get("on_fail")
        self.mode = self.__mode(keys)
        self.retries: int | None = keys.get("retries")
        self.probe = seconds(keys.get("probe", 5))

    def __mode(self, keys: Mapping[str, object]) -> Queue | bool | None:
        """`True` for the sink's own choice, `None` for direct, or the queue."""
        size, batch, wait, full = (keys.get(k) for k in ("queue", "batch", "wait", "full"))
        if size is None and batch is None and wait is None and full is None:
            return True
        if size == 0:
            return None
        base = self.sink.queued() or Queue(1024)
        return Queue(
            int(size) if size is not None else base.size,  # type: ignore[arg-type]
            int(batch) if batch is not None else base.batch,  # type: ignore[arg-type]
            seconds(wait) if wait is not None else base.wait,  # type: ignore[arg-type]
            Full(full) if full is not None else base.full,  # type: ignore[arg-type]
        )


def as_slot(x: Slot | Sink | Callable[..., object] | Mapping[str, object]) -> Slot:
    """A slot from a sink, a function, a config entry or a slot."""
    if isinstance(x, Slot):
        return x
    if isinstance(x, Mapping):
        from ..config import make_sink

        return Slot(make_sink(x))
    return Slot(x)


__all__ = ["Breaker", "Health", "Phase", "Queue", "Slot", "Status", "Tell", "as_slot"]
