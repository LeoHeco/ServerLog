"""What happens when a sink fails: panic, ignore, or raise an alarm that walks tiers of other sinks."""

from collections.abc import Iterable
from dataclasses import dataclass, field
from datetime import UTC, datetime
from enum import StrEnum
from typing import override

from ..layouts.clock import iso
from ..level import Level
from ..record import Record
from ..slot.breaker import Phase
from ..units import seconds, show


class Fallback(StrEnum):
    """What an alarm does when no tier took the incident."""

    PANIC = "panic"
    IGNORE = "ignore"


@dataclass(frozen=True, slots=True)
class Failure:
    """One failed write."""

    sink: str
    error: str
    time: datetime = field(default_factory=lambda: datetime.now(UTC))


@dataclass(slots=True)
class Incident:
    """A sink's failure, from the first error to recovery."""

    sink: str
    kind: str
    phase: Phase
    error: str
    failures: list[Failure] = field(default_factory=list)
    skipped: int = 0
    dropped: int = 0
    since: datetime = field(default_factory=lambda: datetime.now(UTC))

    def __post_init__(self) -> None:
        self.phase = Phase(self.phase)
        if not self.failures and self.phase is not Phase.RESOLVED:
            self.failures.append(Failure(self.sink, self.error))


def describe(i: Incident) -> Record:
    """The default record for an incident."""
    match i.phase:
        case Phase.OPENED:
            level, msg = Level.ERROR, f"sink `{i.sink}` failed: {i.error}"
        case Phase.REMINDER:
            level, msg = Level.ERROR, f"sink `{i.sink}` is still failing: {i.error}"
        case _:
            gone = int((datetime.now(UTC) - i.since).total_seconds())
            level, msg = Level.INFO, f"sink `{i.sink}` works again after {gone}s"
    failures = "; ".join(f"{f.sink}: {f.error}" for f in i.failures)
    fields = (
        ("sink", i.sink),
        ("kind", i.kind),
        ("phase", i.phase.value),
        ("failures", failures),
        ("skipped", i.skipped),
        ("dropped", i.dropped),
        ("since", iso(i.since, 9)),
    )
    return Record(level=level, target="serlog", msg=msg, fields=fields)


class Alarm:
    """Routes incidents to sinks.

    Override `route` to choose the tiers; the alarm task walks them, leaves out the failing sink and any
    that are resting, delivers within a tier at once and stops at the first tier where one sink took it.
    Subclasses cannot cause loops or storms.
    """

    fallback: Fallback = Fallback.IGNORE
    """What to do when no tier took it."""
    remind: float = 3600.0
    """Seconds a sink may stay down before it is reported again."""

    def route(self, incident: Incident) -> list[list[str]]:
        """The tiers for this incident, tried in order; `*` stands for every sink on the log route."""
        raise NotImplementedError

    def describe(self, incident: Incident) -> Record:
        """The record sent; asked again before each tier, so later tiers see earlier failures."""
        return describe(incident)

    def names(self) -> list[str]:
        """Every sink name this alarm may route to, checked at start-up."""
        return []

    def options(self) -> dict[str, object] | None:
        """The config entry that rebuilds this alarm, or `None` when it cannot be exported."""
        return None


class Tiers(Alarm):
    """The built-in alarm: fixed tiers of sink names."""

    def __init__(
        self, tiers: Iterable[Iterable[str] | str] = (("stderr",),), *, on_fail: Fallback | str = Fallback.IGNORE, remind: float | str = 3600
    ) -> None:
        self.tiers = [[t] if isinstance(t, str) else list(t) for t in tiers]
        if not self.tiers or not all(self.tiers):
            raise ValueError("`tiers` needs at least one non-empty tier")
        self.fallback = Fallback(on_fail)
        self.remind = seconds(remind)

    @override
    def route(self, incident: Incident) -> list[list[str]]:
        return [list(t) for t in self.tiers]

    @override
    def names(self) -> list[str]:
        return [n for t in self.tiers for n in t]

    @override
    def options(self) -> dict[str, object]:
        out: dict[str, object] = {"tiers": self.route(None)}  # type: ignore[arg-type]
        if self.fallback is Fallback.PANIC:
            out["on_fail"] = "panic"
        if self.remind != 3600:
            out["remind"] = show(self.remind)
        return out

    def __eq__(self, other: object) -> bool:
        return isinstance(other, Tiers) and self.options() == other.options()

    def __hash__(self) -> int:
        return id(self)


__default: Alarm | None = None


def set_alarm(alarm: Alarm) -> None:
    """Replaces the process-wide default alarm."""
    global __default
    __default = alarm


def current() -> Alarm:
    """The default alarm in force, made on first use."""
    global __default
    if __default is None:
        __default = Tiers()
    return __default


class Current(Alarm):
    """Stands for whatever the default alarm is when an incident happens."""

    @property
    def fallback(self) -> Fallback:  # type: ignore[override]
        return current().fallback

    @property
    def remind(self) -> float:  # type: ignore[override]
        return current().remind

    @override
    def route(self, incident: Incident) -> list[list[str]]:
        return current().route(incident)

    @override
    def describe(self, incident: Incident) -> Record:
        return current().describe(incident)

    @override
    def names(self) -> list[str]:
        return current().names()

    @override
    def options(self) -> dict[str, object] | None:
        return current().options()


DEFAULT = Current()
"""What every slot without an `on_fail` reports to."""

from .task import dropped, post, settle  # noqa: E402
from .walk import Desk, Outcome, sound  # noqa: E402

__all__ = [
    "DEFAULT",
    "Alarm",
    "Desk",
    "Failure",
    "Fallback",
    "Incident",
    "Outcome",
    "Phase",
    "Tiers",
    "current",
    "describe",
    "dropped",
    "post",
    "set_alarm",
    "settle",
    "sound",
]
