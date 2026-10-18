"""The shared vectors in `spec/cases`, which every serlog package must pass."""

import json
import threading
from datetime import UTC, datetime

import pytest
from conftest import vectors

from serlog import Filter, Level, Record, Tiers
from serlog.alarms import Incident, Outcome, sound
from serlog.layouts import PRETTY, Json, Logfmt, Pattern, Schema
from serlog.meta import Meta, set_meta
from serlog.slot.breaker import Breaker, Tell

RENDER = vectors("render")


class Wrapped(Exception):
    """An error with a fixed message, for the `$error` values."""


def value(v: object) -> object:
    match v:
        case {"$error": [first, *rest]}:
            err: BaseException = Wrapped(first)
            top = err
            for msg in rest:
                cause = Wrapped(msg)
                err.__cause__ = cause
                err = cause
            return top
        case dict():
            return {k: value(x) for k, x in v.items()}
        case list():
            return [value(x) for x in v]
    return v


def record(r: dict) -> Record:
    return Record(
        level=Level(r["level"]),
        target=r["target"],
        msg=r["msg"],
        fields=tuple((k, value(v)) for k, v in r["fields"]),
        file=r.get("file"),
        line=r.get("line"),
        thread=threading.current_thread().name,
        time=datetime.fromtimestamp(r["time"], UTC),
    )


@pytest.fixture(autouse=True)
def identity():
    m = RENDER["meta"]
    set_meta(Meta(m["app"], m["env"], m["instance"]))
    yield
    set_meta(Meta.detect())


@pytest.mark.parametrize("case", RENDER["cases"], ids=lambda c: c["record"]["msg"][:12])
def test_renders(case: dict) -> None:
    rec = record(case["record"])
    for p in case["patterns"]:
        assert Pattern(p["pattern"]).render(rec) == p["expect"], p["pattern"]
    for p in case["presets"]:
        got = Pattern(p["name"]).render(rec)
        assert got == p["expect"] if "expect" in p else got.endswith(p["tail"]), (p["name"], got)
    if "logfmt" in case:
        assert Logfmt().render(rec) == case["logfmt"]
    if "pretty" in case:
        assert PRETTY.render(rec).endswith(case["pretty"])
    assert Json().render(rec) == case["json"]
    for s in case["schemas"]:
        got = json.loads(Json(Schema(s["schema"])).render(rec))
        assert list(got) == s["keys"], s["schema"]
        for k, v in s["has"].items():
            assert got[k] == v, (s["schema"], k)


@pytest.mark.parametrize("case", RENDER["filters"], ids=lambda f: f["spec"] or "empty")
def test_filters(case: dict) -> None:
    f = Filter(case["spec"])
    for key, want in (("allow", True), ("deny", False)):
        for lvl, target in case[key]:
            assert f.allows(Level(lvl), target) is want, (case["spec"], lvl, target)
        for msg in case.get("messages", {}).get(key, []):
            assert f.admits(msg) is want, (case["spec"], msg)


@pytest.mark.parametrize("case", vectors("fail")["cases"], ids=lambda c: c["name"])
def test_health_machine(case: dict) -> None:
    b = Breaker(case["probe"], 0.0)
    tell = Tell(case["tell"])
    for i, step in enumerate(case["steps"]):
        now = float(step["at"])
        tried = b.admit(now)
        assert tried is step["tried"], (case["name"], i)
        phase = None
        if tried:
            phase = b.passed() if step["result"] == "ok" else b.failed(now, "boom", case["remind"], tell)
        assert b.health.value == step["health"], (case["name"], i)
        assert (phase.value if phase else None) == step["phase"], (case["name"], i)
    assert b.skipped == case["skipped"]


class Script:
    def __init__(self, case: dict) -> None:
        self.case, self.tried = case, []
        self.lock = threading.Lock()

    def logs(self) -> list[str]:
        return list(self.case["logs"])

    def resting(self, name: str) -> bool:
        return name in self.case["down"]

    def deliver(self, name: str, rec: Record) -> None:
        with self.lock:
            self.tried.append(name)
        if self.case["results"].get(name) != "ok":
            raise OSError("refused")


def by_tier(case: dict, tried: list[str]) -> list[set[str]]:
    left, out = list(tried), []
    for tier in case["tiers"]:
        members = {m for n in tier for m in (case["logs"] if n == "*" else [n])}
        here = {n for n in left if n in members}
        left = [n for n in left if n not in here]
        if here:
            out.append(here)
    return out


@pytest.mark.parametrize("case", vectors("alarm")["cases"], ids=lambda c: c["name"])
def test_alarm_walk(case: dict) -> None:
    desk = Script(case)
    incident = Incident(case["failing"], "custom", "opened", "boom")
    outcome = sound(Tiers(case["tiers"]), incident, desk)
    assert outcome == Outcome(case["outcome"])
    assert by_tier(case, desk.tried) == [set(t) for t in case["tried"]]
    assert [f.sink for f in incident.failures] == case["failures"]
