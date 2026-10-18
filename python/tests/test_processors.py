"""The built-in processors, run on single records."""

import time

from serlog import Dedupe, Exclude, Fields, Level, Limit, Match, Record, Redact, Sample, Sanitise


def rec(msg: str = "x", **kw: object) -> Record:
    return Record(level=kw.pop("level", Level.INFO), target="t", msg=msg, **kw)  # type: ignore[arg-type]


def test_redacts_keys_paths_and_patterns() -> None:
    r = rec(
        "call 13800138000 now", fields=(("pass", "hunter2"), ("req", {"headers": {"authorization": "Bearer x", "accept": "*/*"}}), ("user", "ann"))
    )
    out = Redact(["pass"], paths=["req.headers.authorization"], patterns=[r"\d{11}"])(r)
    assert out is not None
    assert out.field("pass") == "***" and out.field("user") == "ann"
    assert out.field("req") == {"headers": {"authorization": "***", "accept": "*/*"}}
    assert out.msg == "call *** now"
    assert r.field("pass") == "hunter2"


def test_leaves_clean_records_alone() -> None:
    r = rec("fine", fields=(("n", 1),))
    assert Redact(["pass"])(r) is r and Sanitise()(r) is r


def test_sanitises_line_breaks() -> None:
    out = Sanitise()(rec("x\r\ny\x07", fields=(("who", "a\nb"),)))
    assert out.msg == "x\\r\\ny\\u{0007}" and out.field("who") == "a\\nb"


def test_adds_matches_and_excludes() -> None:
    assert Fields(region="cn")(rec()).field("region") == "cn"
    r = rec(level=Level.WARN, tags=("audit",), fields=(("mirror", "debian"),))
    assert Match(tags=["audit"], fields={"mirror": "debian"}, level="warn")(r) is r
    assert Match(tags=["other"])(r) is None
    assert Exclude(tags=["audit"])(r) is None
    assert Exclude(level="error")(r) is r


def test_samples_limits_and_dedupes() -> None:
    s = Sample(2, 3, "1m")
    assert [s(rec("same")) is not None for _ in range(8)] == [True, True, False, False, True, False, False, True]
    lim = Limit(0, 3)
    assert sum(lim(rec()) is not None for _ in range(5)) == 3
    d = Dedupe("200ms")
    assert d(rec("again")) is not None and d(rec("again")) is None and d(rec("again")) is None
    time.sleep(0.25)
    assert d(rec("again")).field("repeated") == 2
