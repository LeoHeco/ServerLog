"""The settings in force as a config tree."""

from .. import alarms, hub
from ..compat import Compat, compat
from ..errors import ConfigError
from ..layouts import INHERIT, LOGFMT, PRETTY, JsonLayout, Pattern, Schema
from ..level import Level
from ..meta import meta
from ..queues import Full
from ..slot.live import Live
from ..units import show

__named: dict[str, alarms.Alarm] = {}


def name_alarms(named: dict[str, alarms.Alarm]) -> None:
    """Remembers the names alarms were given in config."""
    __named.clear()
    __named.update(named)


def __steps(steps: tuple[object, ...], whose: str) -> list[object]:
    out = []
    for s in steps:
        o = s.options() if hasattr(s, "options") else None  # type: ignore[union-attr]
        if o is None:
            raise ConfigError(f"a processor in {whose} cannot be exported")
        out.append(o)
    return out


def __layout(x: Live, out: dict[str, object]) -> None:
    lay = x.slot.layout
    match lay:
        case None:
            return
        case _ if lay is INHERIT:
            return
        case Pattern():
            out["pattern"] = lay.source
        case _ if lay is LOGFMT:
            out["format"] = "logfmt"
        case _ if lay is PRETTY:
            out["format"] = "pretty"
        case JsonLayout():
            j = lay.json
            out["format"] = "json"
            if j.schema is not Schema.SERLOG:
                out["schema"] = j.schema.value
            if j.keys:
                out["keys"] = j.keys
            if j.levels:
                out["levels"] = j.levels.value
            if j.nest:
                out["nest"] = j.nest
        case _:
            raise ConfigError(f"the layout of sink `{x.name}` cannot be exported")


def __on_fail(policy: object, extra: dict[str, alarms.Alarm]) -> str | None:
    match policy:
        case "panic" | "ignore":
            return policy  # type: ignore[return-value]
        case _ if policy is alarms.DEFAULT:
            return None
    for name, a in (*__named.items(), *extra.items()):
        if a is policy:
            return name
    name = f"alarm-{len(extra) + 1}"
    extra[name] = policy  # type: ignore[assignment]
    return name


def __sink(x: Live, extra: dict[str, alarms.Alarm]) -> dict[str, object]:
    c, s = x.core, x.slot
    own = c.sink.options()
    if own is None:
        raise ConfigError(f"sink `{x.name}` cannot be exported")
    out: dict[str, object] = {"kind": c.kind, "name": x.name, **own}
    if x.filter is not None:
        out["filter"] = str(x.filter)
    if steps := __steps(x.process, f"sink `{x.name}`"):
        out["process"] = steps
    __layout(x, out)
    if s.colour.value != "auto":
        out["colour"] = s.colour.value
    if over := s.theme.overrides:
        out["colours"] = {Level(k).name: v.value for k, v in over.items()}
    if x.alarm_only:
        out["alarm_only"] = True
    if (policy := __on_fail(c.on_fail, extra)) is not None:
        out["on_fail"] = policy
    if s.mode is not True:
        q = x.queue
        if q is None:
            out["queue"] = 0
        else:
            out |= {"queue": q.size, "batch": q.batch, "wait": show(q.wait)}
            if q.full is Full.BLOCK:
                out["full"] = "block"
    if s.retries is not None:
        out["retries"] = s.retries
    if s.probe != 5:
        out["probe"] = show(s.probe)
    return out


def tree() -> dict[str, object]:
    """The settings in force."""
    m = meta()
    out: dict[str, object] = {k: v for k, v in (("app", m.app), ("env", m.env), ("instance", m.instance)) if v}
    if compat() is not Compat.SERLOG:
        out["compat"] = compat().value
    if defined := Level.defined():
        out["levels"] = {n: v if c.value == "plain" else {"value": v, "colour": c.value} for n, (v, c) in defined.items()}
    out["filter"] = str(hub.filter())
    if steps := __steps(hub.chain(), "the global chain"):
        out["process"] = steps
    extra: dict[str, alarms.Alarm] = {}
    out["sinks"] = [__sink(x, extra) for x in hub.table()]
    found: dict[str, object] = {}
    current = alarms.current().options()
    if current is None:
        raise ConfigError("the default alarm cannot be exported")
    if current != alarms.Tiers().options():
        found["default"] = current
    for name, a in (*__named.items(), *extra.items()):
        o = a.options()
        if o is None:
            raise ConfigError(f"alarm `{name}` cannot be exported")
        found[name] = o
    if found:
        out["alarms"] = found
    return out
