"""Making records: messages, t-strings, typed fields, tags, the caller, scopes, laziness and catching."""

import asyncio

import pytest

import serlog
from serlog import Level, Memory


@pytest.fixture
def mem(clean):
    serlog.set_level("trace")
    m = Memory(100)
    serlog.add(m, filter="calls=trace,tests=trace,test_calls=trace")
    return m


def last(mem: Memory, msg: str) -> serlog.Record:
    return mem.find(lambda r: r.msg == msg)[-1]


def test_messages_fields_and_the_caller(mem: Memory) -> None:
    sync = serlog.get("calls", mirror="debian")
    sync.info("synced {} files", 42, tries=3, ratio=0.5, ok=True, tags=["sync"])
    r = last(mem, "synced 42 files")
    assert r.fields == (("mirror", "debian"), ("tries", 3), ("ratio", 0.5), ("ok", True))
    assert r.tags == ("sync",)
    assert r.func == "test_messages_fields_and_the_caller"
    assert r.file == __file__ and r.module == __name__


def test_t_strings_become_fields(mem: Memory) -> None:
    n, name = 3, "debian"
    serlog.get("calls").warn(t"synced {n:03d} files for {name!r}")
    r = last(mem, "synced 003 files for 'debian'")
    assert r.fields == (("n", 3), ("name", "debian"))


def test_module_calls_use_the_calling_module(mem: Memory) -> None:
    serlog.info("plain call")
    assert last(mem, "plain call").target == __name__


def test_levels_and_aliases(mem: Memory) -> None:
    log = serlog.get("calls")
    log.warning("w")
    log.critical("c")
    log.log("debug", "d")
    assert [last(mem, m).level for m in "wcd"] == [Level.WARN, Level.FATAL, Level.DEBUG]


def test_arguments_are_not_worked_out_below_the_level(mem: Memory) -> None:
    serlog.set_level("info")
    hit = []
    serlog.get("calls").debug("{}", serlog.lazy(lambda: hit.append(1)))
    assert hit == []
    serlog.get("calls").info("{}", serlog.lazy(lambda: "worked"))
    assert last(mem, "worked")


def test_bind_spans_threads_and_tasks(mem: Memory) -> None:
    with serlog.bind(request=7):
        serlog.get("calls").info("inside", extra=1)

        async def task() -> None:
            serlog.get("calls").info("in a task")

        asyncio.run(task())
    serlog.get("calls").info("outside")
    assert last(mem, "inside").fields == (("request", 7), ("extra", 1))
    assert last(mem, "in a task").fields == (("request", 7),)
    assert last(mem, "outside").fields == ()


def test_children_add_fields(mem: Memory) -> None:
    serlog.get("calls", a=1).child(b=2).info("child")
    assert last(mem, "child").fields == (("a", 1), ("b", 2))


def test_exceptions_carry_the_error(mem: Memory) -> None:
    try:
        raise ValueError("bad input")
    except ValueError:
        serlog.get("calls").exception("parse failed")
    r = last(mem, "parse failed")
    assert isinstance(r.field("error"), ValueError)


def test_catch_logs_and_raises_or_swallows(mem: Memory) -> None:
    @serlog.catch(target="calls")
    def boom() -> None:
        raise RuntimeError("no")

    with pytest.raises(RuntimeError):
        boom()

    @serlog.catch(reraise=False, target="calls", level="warn")
    async def quiet() -> None:
        raise KeyError("k")

    asyncio.run(quiet())
    with serlog.catch(reraise=False, target="calls"):
        raise OSError("disk")
    msgs = [r.msg for r in mem.records if r.target == "calls" and "raised" in r.msg]
    assert len(msgs) == 3
    assert any("RuntimeError: no" in m for m in msgs)
    assert any(r.level == Level.WARN for r in mem.records if "KeyError" in r.msg)
