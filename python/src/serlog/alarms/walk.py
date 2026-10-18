"""Walking an alarm's tiers: the same rules in every language, checked by `spec/cases/alarm.json`."""

from concurrent.futures import ThreadPoolExecutor
from enum import StrEnum
from typing import TYPE_CHECKING, Protocol

from ..record import Record

if TYPE_CHECKING:
    from . import Alarm, Incident


class Desk(Protocol):
    """What the walk needs from the sinks around it."""

    def logs(self) -> list[str]:
        """Names of the sinks on the log route, which `*` stands for."""
        ...

    def resting(self, name: str) -> bool:
        """Whether the sink is down and not yet due for a probe."""
        ...

    def deliver(self, name: str, rec: Record) -> None:
        """Writes `rec` to the named sink straight away; raises what went wrong."""
        ...


class Outcome(StrEnum):
    """How a walk ended."""

    HEARD = "heard"
    UNHEARD = "unheard"


def sound(alarm: Alarm, incident: Incident, desk: Desk) -> Outcome:
    """Delivers `incident` through `alarm`'s tiers, adding every failed delivery to it."""
    from . import Failure

    out = {incident.sink}
    for tier in alarm.route(incident):
        names: list[str] = []
        for n in tier:
            for m in desk.logs() if n == "*" else [n]:
                if m not in out and m not in names and not desk.resting(m):
                    names.append(m)
        if not names:
            continue
        rec = alarm.describe(incident)

        def one(name: str, rec: Record = rec) -> tuple[str, str | None]:
            try:
                desk.deliver(name, rec)
            except Exception as e:
                return name, str(e) or type(e).__name__
            return name, None

        if len(names) == 1:
            results = [one(names[0])]
        else:
            with ThreadPoolExecutor(len(names), thread_name_prefix="serlog-alarm") as pool:
                results = list(pool.map(one, names))
        heard = False
        for name, error in results:
            if error is None:
                heard = True
            else:
                incident.failures.append(Failure(name, error))
            out.add(name)
        if heard:
            return Outcome.HEARD
    return Outcome.UNHEARD
