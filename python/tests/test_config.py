"""Config files and start-up: every key, custom kinds, errors, export in three formats, reloading and profiles."""

import json
import logging
import warnings

import pytest

import serlog
from serlog import ConfigError, KindError, Memory
from serlog.config import read
from serlog.start import plan

CONFIG = """
app: demo
level: trace
watch: 1s
levels:
  notice: { value: 35, colour: blue }
process:
  - fields: { region: cn }
redact: [pass]
alarms:
  ops:
    tiers: [[oncall], [stderr]]
    remind: 30m
sinks:
  - { kind: captured, slot: all, name: all, format: json }
  - { kind: captured, slot: notices, level: notice, pattern: "{level}:{msg}" }
  - { kind: captured, slot: oncall, name: oncall, alarm_only: true }
  - { kind: file, name: app, path: "${SERLOG_TEST_DIR}/app.log", max_size: 1MiB, keep: 3, on_fail: ops, queue: 100, wait: 50ms }
"""

SCRIPT = """
import json, os, pathlib, time
import serlog

captured = {}

@serlog.kind("captured")
class Captured(serlog.Memory):
    kind = "captured"
    def __init__(self, slot, **rest):
        super().__init__(100, **rest)
        self.slot_name = slot
        captured[slot] = self
    def options(self):
        return {"slot": self.slot_name}

path = pathlib.Path("serlog.yaml")
path.write_text(os.environ["CONFIG"])
serlog.load(path)
serlog.info("hello", **{"pass": "secret"})
serlog.log(35, "notable")
serlog.flush()
out = {"all": captured["all"].lines, "notices": captured["notices"].lines, "names": [s.name for s in serlog.status()]}
out["file"] = pathlib.Path(os.environ["SERLOG_TEST_DIR"], "app.log").read_text()
out["yaml"] = serlog.dump("yaml")
trees = [serlog.config.read(serlog.dump(f), f) for f in ("yaml", "json", "toml")]
out["same"] = trees[0] == trees[1] == trees[2]
time.sleep(1.1)
path.write_text("level: info\\nsinks:\\n  - { kind: captured, slot: after, name: after }\\n")
time.sleep(2.5)
serlog.info("reloaded")
serlog.flush()
out["after"] = captured["after"].lines
out["names_after"] = [s.name for s in serlog.status()]
print(json.dumps(out))
"""


def test_loads_dumps_and_reloads(run, tmp_path) -> None:
    stdout, _, _ = run(SCRIPT, CONFIG=CONFIG, SERLOG_TEST_DIR=str(tmp_path))
    out = json.loads(stdout.strip().splitlines()[-1])
    assert any('"msg":"hello"' in line and '"region":"cn"' in line and '"pass":"***"' in line for line in out["all"]), out["all"]
    assert out["notices"] == ["NOTICE:notable"]
    assert "hello" in out["file"]
    assert out["names"] == ["all", "captured-1", "oncall", "app"]
    for want in ("notice", "region", "redact", "ops", "remind: 30m", "max_size: 1MiB", "on_fail: ops", "queue: 100"):
        assert want in out["yaml"], want
    assert out["same"]
    assert any(line.endswith("reloaded") for line in out["after"])
    assert out["names_after"] == ["after"]


def test_reads_the_same_tree_from_every_format() -> None:
    y = read("filter: info\nsinks:\n  - kind: console\n    stream: stderr\n", "yaml")
    j = read('{"filter":"info","sinks":[{"kind":"console","stream":"stderr"}]}', "json")
    t = read('filter = "info"\n[[sinks]]\nkind = "console"\nstream = "stderr"\n', "toml")
    assert y == j == t


@pytest.mark.parametrize(
    ("tree", "says"),
    [
        ({"sinks": [{"kind": "console", "colr": "never"}]}, "unknown key `colr`"),
        ({"bogus": 1}, "unknown key `bogus`"),
        ({"sinks": [{"kind": "console", "level": "info", "filter": "info"}]}, "not both"),
        ({"sinks": [{"kind": "console", "on_fail": "pager"}]}, "not in `alarms`"),
        ({"sinks": [{"kind": "console", "format": "xml"}]}, "format"),
        ({"alarms": {"panic": {"tiers": [["stderr"]]}}}, "cannot name"),
        ({"process": [{"shout": {}}]}, "shout"),
        ({"alarms": {"ops": {"tiers": [["nobody"]]}}, "sinks": [{"kind": "memory", "on_fail": "ops"}]}, "nobody"),
    ],
)
def test_reports_mistakes_by_name(tree: dict, says: str) -> None:
    with pytest.raises(serlog.SerlogError, match=says):
        plan(tree, ())


def test_unknown_kinds_and_variables() -> None:
    with pytest.raises(KindError):
        plan({"sinks": [{"kind": "kafka"}]}, ())
    with pytest.raises(ConfigError, match="SERLOG_SURELY_UNSET"):
        serlog.config.expand({"x": "${SERLOG_SURELY_UNSET}"})


def test_zero_config_follows_the_environment(run) -> None:
    out, _, _ = run("import serlog\nserlog.init()\nserlog.debug('quiet')\nserlog.warn('loud', n=1)\n", SERLOG_FILTER="warn", SERLOG_FORMAT="json")
    rec = json.loads(out.strip())
    assert (rec["level"], rec["msg"], rec["n"]) == ("warn", "loud", 1)


def test_profiles_change_pattern_stream_and_level(run) -> None:
    out, err, _ = run("import serlog\nserlog.init(compat='python')\nserlog.info('hidden')\nserlog.warn('shown')\nserlog.fatal('worse')\n")
    assert out == ""
    assert err.splitlines() == ["WARNING:__main__:shown", "CRITICAL:__main__:worse"]


def test_an_empty_config_file_starts_the_console(run) -> None:
    out, _, _ = run("import pathlib, serlog\npathlib.Path('serlog.yaml').write_text('')\nserlog.init()\nserlog.info('up')\n")
    assert out.endswith("__main__: up\n"), out


def test_init_runs_once(run) -> None:
    _, err, code = run("import serlog\nserlog.init()\nserlog.init()\n", ok=False)
    assert code != 0 and "already initialised" in err


def test_a_background_panic_exits_70(run) -> None:
    code = """
import serlog
class Bad(serlog.Sink):
    def emit(self, entry):
        raise OSError("gone")
serlog.init(sinks=[Bad(queue=10, wait=0.01, on_fail="panic", retries=0)])
serlog.info("x")
import time; time.sleep(2)
"""
    _, err, rc = run(code, ok=False)
    assert rc == 70 and "gone" in err


def test_standard_logging_and_warnings_come_through(clean) -> None:
    mem = Memory(10)
    serlog.add(mem, filter="std=trace,py.warnings=trace")
    serlog.set_level("trace")
    serlog.capture()
    logging.getLogger("std").warning("from logging %s", 1, extra={"mirror": "debian"})
    warnings.warn("careful", stacklevel=1)
    r = mem.find(lambda r: r.msg == "from logging 1")[0]
    assert r.level == serlog.Level.WARN and r.field("mirror") == "debian"
    assert any("careful" in r.msg for r in mem.records)


def test_the_schema_accepts_the_spec_example() -> None:
    from conftest import SPEC
    from jsonschema import Draft202012Validator

    schema = json.loads((SPEC / "serlog.schema.json").read_text(encoding="utf-8"))
    example = (SPEC / "README.md").read_text(encoding="utf-8").split("```yaml\n", 1)[1].split("```", 1)[0]
    v = Draft202012Validator(schema)
    assert not list(v.iter_errors(read(example, "yaml")))
    assert list(v.iter_errors({"sinks": [{"kind": "console", "colr": "x"}]}))
