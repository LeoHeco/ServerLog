"""Start-up: one call, the same keys as a config file, all checked before anything changes."""

import atexit
import os
from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Literal, TypedDict, Unpack

from . import alarms, hub
from .compat import Compat, set_compat
from .errors import ConfigError, Twice
from .filter import Filter
from .level import Level, Levelish
from .meta import Meta, set_meta
from .processors import Redact, Step
from .sink import Sink
from .slot import Slot, as_slot
from .slot.live import Live


class Settings(TypedDict, total=False):
    """What `init` takes: the keys of a config file."""

    app: str
    env: str
    instance: str
    compat: Compat | str
    level: Levelish
    filter: str | Filter
    filter_env: str
    levels: Mapping[str, int | Mapping[str, object]]
    process: Iterable[Step | str | Mapping[str, object]]
    redact: Iterable[str]
    sinks: Iterable[Sink | Slot | Mapping[str, object]]
    alarms: Mapping[str, alarms.Alarm | Mapping[str, object]]
    watch: float | str


__KEYS = Settings.__required_keys__ | Settings.__optional_keys__
__started = [False]


@dataclass(slots=True)
class Plan:
    """Everything checked and built, ready to swap in."""

    meta: Meta
    compat: Compat
    filter: Filter
    process: tuple[Step, ...]
    live: list[Live]
    alarm: alarms.Alarm | None
    named: dict[str, alarms.Alarm]


def __console() -> Slot:
    from .sinks import Console

    fmt = os.environ.get("SERLOG_FORMAT") or None
    if fmt not in (None, "text", "json", "logfmt", "pretty"):
        raise ConfigError(f"SERLOG_FORMAT {fmt!r} is not text, json, logfmt or pretty")
    return Slot(Console(), format=fmt) if fmt else Slot(Console())


def plan(cfg: Mapping[str, object], alongside: Iterable[Live]) -> Plan:
    """Checks `cfg` and builds its slots, keeping nothing on error."""
    from .config import make_alarm, make_step

    if unknown := set(cfg) - __KEYS:
        raise ConfigError(f"unknown key `{sorted(unknown)[0]}`")
    for name, spec in dict(cfg.get("levels") or {}).items():  # type: ignore[call-overload]
        value, colour = (spec, "plain") if isinstance(spec, int) else (spec["value"], spec.get("colour", "plain"))
        Level.define(name, value, colour)
    env = os.environ.get("SERLOG_COMPAT", "").strip()
    profile = Compat(env or cfg.get("compat") or Compat.SERLOG)
    var = cfg.get("filter_env")
    names: tuple[str, ...] = (str(var),) if var else profile.env
    found = next((val for n in names if (val := os.environ.get(n, "").strip())), None)
    spec = found or cfg.get("filter")
    level = cfg.get("level")
    f = (
        Filter(spec)
        if isinstance(spec, str)
        else spec
        if isinstance(spec, Filter)
        else Filter(int(Level(level if level is not None else profile.level)))
    )  # type: ignore[arg-type]
    steps = [make_step(s) if isinstance(s, str | Mapping) else s for s in cfg.get("process") or ()]  # type: ignore[attr-defined]
    if redact := list(cfg.get("redact") or ()):  # type: ignore[call-overload]
        steps.append(Redact(redact))
    named = {n: a if isinstance(a, alarms.Alarm) else make_alarm(n, a) for n, a in dict(cfg.get("alarms") or {}).items()}  # type: ignore[call-overload]
    default = named.pop("default", None)
    slots = [as_slot(s) for s in cfg.get("sinks") or ()] or [__console()]  # type: ignore[attr-defined]
    live = hub.prepare(slots, alongside, named)
    known = hub.known(live)
    for whose, a in (("the default alarm", default), *((f"the alarm of `{x.name}`", x.core.on_fail) for x in live)):
        if isinstance(a, alarms.Alarm) and a is not alarms.DEFAULT and (bad := next((n for n in a.names() if n not in known), None)):
            raise ConfigError(f"{whose} routes to `{bad}`, which is not a sink")
    m = Meta.detect()
    ident = Meta(str(cfg.get("app", m.app)), str(cfg.get("env", m.env)), str(cfg.get("instance", m.instance)))
    return Plan(ident, profile, f, tuple(steps), live, default, named)


def apply(p: Plan, replace: bool = False) -> None:
    """Puts a plan in force, adding to the slots or replacing them."""
    from .config.export import name_alarms

    set_meta(p.meta)
    set_compat(p.compat)
    hub.set_filter(p.filter)
    hub.set_process(p.process)
    if p.alarm is not None:
        alarms.set_alarm(p.alarm)
    name_alarms(p.named)
    if replace:
        hub.replace(p.live)
    else:
        hub.install(p.live)


def start(cfg: Mapping[str, object]) -> None:
    """Starts from settings already in hand, as `load` does."""
    if __started[0]:
        raise Twice
    p = plan(cfg, hub.table())
    __started[0] = True
    apply(p)
    atexit.register(hub.flush)


def init(**cfg: Unpack[Settings]) -> None:
    """Starts serlog once, from keyword settings that are also the keys of a config file.

    With no settings it reads `SERLOG_CONFIG`, or the first of `serlog.yaml`, `serlog.yml`, `serlog.json`
    and `serlog.toml` in the working directory; failing that it starts a console sink.
    """
    if __started[0]:
        raise Twice
    if not cfg:
        path = os.environ.get("SERLOG_CONFIG") or next(
            (n for n in ("serlog.yaml", "serlog.yml", "serlog.json", "serlog.toml") if Path(n).exists()), None
        )
        if path:
            from .config import load

            return load(path)
    return start(cfg)


def started() -> Literal[True, False]:
    """Whether `init` ran."""
    return __started[0]


if hasattr(os, "register_at_fork"):
    os.register_at_fork(after_in_child=hub.forget)
