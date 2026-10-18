"""Slots at run time: names, claims, start-up checks, queues, health, failure policies, alarms and loops."""

import pytest

import serlog
from serlog import Entry, Health, Memory, NameTaken, NotReady, Sink, Tiers


class Flaky(Sink):
    kind = "flaky"

    def __init__(self, **slot) -> None:
        super().__init__(**slot)
        self.fail, self.seen = False, 0

    def emit(self, entry: Entry) -> None:
        self.seen += 1
        if self.fail:
            raise OSError("boom")


def status(name: str) -> serlog.Status:
    return next(s for s in serlog.status() if s.name == name)


def only(target: str) -> str:
    return f"{target}=trace"


@pytest.fixture(autouse=True)
def quiet(clean):
    serlog.set_level("trace")
    if not any(s.name == "main" for s in serlog.status()):
        serlog.add(Memory(1, name="main", filter="off"))


def test_a_failure_is_told_once_and_its_recovery_after() -> None:
    alarms = Memory(10)
    serlog.add(alarms, name="alarms1", alarm_only=True)
    flaky = Flaky(name="flaky1", filter=only("t1"), probe=0, on_fail=Tiers([["alarms1"]]))
    flaky.fail = True
    serlog.add(flaky)
    for _ in range(3):
        serlog.get("t1").info("write")
    serlog.flush()
    assert flaky.seen == 3
    assert status("flaky1").health is Health.DOWN
    told = alarms.records
    assert len(told) == 1
    assert told[0].level == serlog.Level.ERROR
    assert told[0].msg == "sink `flaky1` failed: boom"
    assert told[0].field("phase") == "opened"
    flaky.fail = False
    serlog.get("t1").info("write")
    serlog.flush()
    assert status("flaky1").health is Health.UP
    assert [r.field("phase") for r in alarms.records] == ["opened", "resolved"]


def test_a_down_sink_is_skipped_until_its_probe() -> None:
    flaky = Flaky()
    flaky.fail = True
    serlog.add(flaky, name="flaky2", filter=only("t2"), probe="1h", on_fail="ignore")
    for _ in range(3):
        serlog.get("t2").info("write")
    assert flaky.seen == 1
    s = status("flaky2")
    assert (s.health, s.skipped, s.error) == (Health.DOWN, 2, "boom")


def test_the_panic_policy_raises_in_the_caller_every_time() -> None:
    flaky = Flaky()
    flaky.fail = True
    serlog.add(flaky, name="flaky3", filter=only("t3"), on_fail="panic")
    for _ in range(2):
        with pytest.raises(serlog.SerlogError, match="flaky3"):
            serlog.get("t3").error("write")
    assert flaky.seen == 2


def test_names_claims_and_checks_are_enforced(tmp_path) -> None:
    a = serlog.add(Memory(1, filter="off"))
    b = serlog.add(Memory(1, filter="off"))
    assert a.startswith("memory-") and b.startswith("memory-") and a != b
    with pytest.raises(NameTaken):
        serlog.add(Memory(1), name="main")
    with pytest.raises(NameTaken):
        serlog.add(Memory(1), name="*")
    path = tmp_path / "one.log"
    serlog.add(serlog.File(path, name="one", filter="off"))
    with pytest.raises(serlog.Claimed):
        serlog.add(serlog.File(path, name="two"))
    with pytest.raises(NotReady):
        serlog.add(serlog.File(tmp_path, name="three"))
    assert serlog.remove("one") and not serlog.remove("one")


def test_a_queued_slot_writes_in_the_background() -> None:
    mem = Memory(100)
    serlog.add(mem, name="queued", filter=only("t5"), queue=100, wait="20ms")
    for i in range(5):
        serlog.get("t5").info("queued", n=i)
    serlog.flush()
    assert len(mem) == 5


def test_slots_lay_out_and_process_on_their_own() -> None:
    js, text = Memory(10), Memory(10)
    serlog.add(js, name="json", filter=only("t6"), format="json")
    serlog.add(text, name="text", filter=only("t6"), pattern="{level}:{msg}", process=[{"match": {"tags": ["keep"]}}])
    log = serlog.get("t6")
    log.info("kept", tags=["keep"])
    log.info("dropped by text")
    assert len(js) == 2 and js.lines[0].startswith('{"time":')
    assert text.lines == ["INFO:kept"]


class Loud(Sink):
    def __init__(self) -> None:
        super().__init__()
        self.seen = 0

    def emit(self, entry: Entry) -> None:
        self.seen += 1
        serlog.get("t7").info("from inside the sink")


def test_records_made_inside_a_sink_are_suppressed() -> None:
    loud = Loud()
    serlog.add(loud, name="loud", filter=only("t7"))
    serlog.get("t7").info("outside")
    assert loud.seen == 1
    assert status("loud").suppressed == 1


def test_functions_are_sinks() -> None:
    got = []
    serlog.add(lambda e: got.append(e.line), name="fn", filter=only("t8"), pattern="{msg}")
    serlog.get("t8").warn("hi")
    assert got == ["hi"]


def test_removing_a_slot_writes_its_queue_and_closes_its_sink(tmp_path) -> None:
    mem = Memory(10)
    serlog.add(mem, name="leaving", filter=only("t11"), queue=10, wait="1h")
    for _ in range(3):
        serlog.get("t11").info("queued")
    path = tmp_path / "gone.log"
    serlog.add(serlog.File(path, name="gone", filter="off"))
    assert serlog.remove("leaving") and serlog.remove("gone")
    assert len(mem) == 3
    path.unlink()
