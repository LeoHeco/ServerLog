"""The reference sinks, checked against the file system and stand-in Loki and SMTP servers."""

import gzip
import json
import os
import socketserver
import sys
import threading
import time
from datetime import UTC, datetime, timedelta
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import ClassVar

import pytest

from serlog import ConfigError, Entry, File, Level, Loki, Mail, Pattern, Record, Rejected

MSG = Pattern("{msg}")


def put(f: File, msg: str, at: datetime | None = None, level: Level = Level.INFO) -> None:
    f.emit(Entry(Record(level=level, target="t", msg=msg, time=at or datetime.now(UTC)), MSG))


def names(d: Path) -> list[str]:
    return sorted(p.name for p in d.iterdir())


def text(p: Path) -> str:
    return gzip.decompress(p.read_bytes()).decode() if p.suffix == ".gz" else p.read_text(encoding="utf-8")


def test_rolls_by_size_keeps_the_newest_and_gzips(tmp_path: Path) -> None:
    f = File(tmp_path / "app.log", max_size=150, keep=2, gzip=True)
    f.ready()
    start = datetime.now(UTC)
    for i in range(12):
        put(f, f"line {i:02d} {'x' * 60}", start + timedelta(seconds=i))
        f.flush()
    rolled = [n for n in names(tmp_path) if n.endswith(".log.gz")]
    assert len(rolled) == 2 and len(names(tmp_path)) == 3
    assert "line 11" in text(tmp_path / "app.log")
    joined = "".join(text(tmp_path / n) for n in rolled)
    assert "line 07" in joined and "line 09" in joined and "line 05" not in joined


def test_rotates_when_the_period_turns(tmp_path: Path) -> None:
    f = File(tmp_path / "day.log", rotate="day")
    f.ready()
    now = datetime.now(UTC)
    put(f, "today", now)
    put(f, "still today", now)
    put(f, "tomorrow", now + timedelta(hours=25))
    f.flush()
    old = [n for n in names(tmp_path) if n != "day.log"]
    assert len(old) == 1 and "still today" in text(tmp_path / old[0])
    assert text(tmp_path / "day.log") == "tomorrow\n"


def test_names_rolled_files_from_a_template(tmp_path: Path) -> None:
    f = File(tmp_path / "svc.log", max_size=10, roll_name="{stem}-old.{ext}")
    f.ready()
    for m in ("first line", "second line", "third line"):
        put(f, m)
    f.flush()
    assert names(tmp_path) == ["svc-old-1.log", "svc-old.log", "svc.log"]


def test_cleans_up_by_total_size_and_leaves_others(tmp_path: Path) -> None:
    (tmp_path / "big.yaml").write_text("x")
    f = File(tmp_path / "big.log", max_size=100, max_total=250)
    f.ready()
    for i in range(8):
        put(f, f"{i} {'y' * 97}")
        f.flush()
    rolled = [tmp_path / n for n in names(tmp_path) if n not in ("big.log", "big.yaml")]
    assert sum(p.stat().st_size for p in rolled) <= 250
    assert (tmp_path / "big.yaml").exists()


@pytest.mark.skipif(sys.platform == "win32", reason="Windows does not let others move a file Python holds open")
def test_reopens_after_the_file_is_moved_away(tmp_path: Path) -> None:
    path = tmp_path / "moved.log"
    f = File(path, reopen=True)
    f.ready()
    put(f, "before")
    f.flush()
    f.flush()
    path.rename(tmp_path / "moved.log.1")
    time.sleep(1.1)
    put(f, "after")
    f.flush()
    assert text(path) == "after\n"


def test_truncates_and_makes_directories(tmp_path: Path) -> None:
    path = tmp_path / "t.log"
    path.write_text("old\n")
    f = File(path, append=False)
    f.ready()
    put(f, "new")
    f.flush()
    assert text(path) == "new\n"
    File(tmp_path / "a" / "b" / "c.log").ready()
    assert (tmp_path / "a" / "b" / "c.log").exists()
    with pytest.raises(ConfigError):
        File(tmp_path / "x" / "y.log", mkdir=False).ready()


def test_buffers_until_an_error(tmp_path: Path) -> None:
    path = tmp_path / "buf.log"
    f = File(path, buffer="64KiB", flush_every="1h")
    f.ready()
    put(f, "first")
    put(f, "second")
    assert text(path) == "first\n"
    put(f, "bad", level=Level.ERROR)
    assert text(path) == "first\nsecond\nbad\n"


class Stand(BaseHTTPRequestHandler):
    answers: ClassVar[list[int]] = []
    seen: ClassVar[list[tuple[str, str, dict, str]]] = []

    def __reply(self, body: str) -> None:
        self.seen.append((self.command, self.path, dict(self.headers), body))
        self.send_response(self.answers.pop(0) if self.answers else 204)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_POST(self) -> None:
        raw = self.rfile.read(int(self.headers["Content-Length"]))
        self.__reply(gzip.decompress(raw).decode())

    def do_GET(self) -> None:
        self.__reply("")

    def log_message(self, format: str, *args: object) -> None:
        pass


@pytest.fixture
def loki_url():
    server = HTTPServer(("127.0.0.1", 0), Stand)
    Stand.answers, Stand.seen = [], []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    yield f"http://127.0.0.1:{server.server_port}/loki/api/v1/push"
    server.shutdown()


def test_loki_pushes_one_stream_per_level(loki_url: str) -> None:
    loki = Loki(loki_url, labels={"team": "mirrors"})
    lay = loki.default_layout()
    recs = [
        Record(level=Level.INFO, target="sync", msg="started", file="a.py", line=3, fields=(("mirror", "debian"), ("tries", 3))),
        Record(level=Level.ERROR, target="sync", msg='failed "rsync"'),
        Record(level=Level.INFO, target="sync", msg="retrying"),
    ]
    loki.emit_batch([Entry(r, lay) for r in recs])
    _, path, headers, body = Stand.seen[0]
    assert path == "/loki/api/v1/push"
    assert headers["Content-Encoding"] == "gzip" and headers["Content-Type"] == "application/json"
    info, err = json.loads(body)["streams"]
    assert (info["stream"]["level"], info["stream"]["team"]) == ("info", "mirrors")
    assert [v[1] for v in info["values"]] == ["started", "retrying"]
    assert info["values"][0][2] == {"target": "sync", "file": "a.py:3", "mirror": "debian", "tries": "3"}
    assert err["values"][0][1] == 'failed "rsync"'


def test_loki_tells_retryable_answers_from_refusals(loki_url: str) -> None:
    Stand.answers = [503, 400]
    loki = Loki(loki_url)
    e = Entry(Record(level=Level.INFO, target="t", msg="x"), loki.default_layout())
    with pytest.raises(OSError, match="503"):
        loki.emit(e)
    with pytest.raises(Rejected):
        loki.emit(e)


def test_loki_checks_its_url_and_asks_ready(loki_url: str) -> None:
    with pytest.raises(ConfigError):
        Loki("ftp://x").ready()
    Loki(loki_url, connect=True).ready()
    assert Stand.seen[-1][:2] == ("GET", "/ready")


class Smtp(socketserver.StreamRequestHandler):
    data: ClassVar[list[str]] = []

    def handle(self) -> None:
        self.wfile.write(b"220 test\r\n")
        while line := self.rfile.readline().decode():
            cmd = line.upper()
            if cmd.startswith("EHLO"):
                self.wfile.write(b"250-test\r\n250 OK\r\n")
            elif cmd.startswith("DATA"):
                self.wfile.write(b"354 go on\r\n")
                body = []
                while (part := self.rfile.readline().decode()) not in (".\r\n", ""):
                    body.append(part)
                self.data.append("".join(body))
                self.wfile.write(b"250 queued\r\n")
            elif cmd.startswith("QUIT"):
                self.wfile.write(b"221 bye\r\n")
                return
            else:
                self.wfile.write(b"250 OK\r\n")


def test_mail_sends_one_message_per_batch() -> None:
    server = socketserver.ThreadingTCPServer(("127.0.0.1", 0), Smtp)
    Smtp.data = []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    m = Mail("127.0.0.1", port=server.server_address[1], security="none", sender="bot@example.org", to="ops@example.org")
    m.ready()
    assert not m.enabled(Level.WARN, "x") and m.enabled(Level.ERROR, "x")
    lay = Pattern("{level} {msg}")
    m.emit_batch(
        [
            Entry(Record(level=Level.ERROR, target="sync", msg="disk <full>"), lay),
            Entry(Record(level=Level.FATAL, target="sync", msg="mirror stalled"), lay),
        ]
    )
    server.shutdown()
    data = Smtp.data[0]
    assert "2 FATAL records" in data
    assert "ERROR disk <full>" in data and "disk &lt;full&gt;" in data
    assert "mirror stalled" in data


def test_mail_refuses_to_start_without_addresses() -> None:
    for m in (Mail("h", to="ops@example.org"), Mail("h", sender="bot@example.org"), Mail("h", user="bot@example.org", to="ops@example.org")):
        with pytest.raises(ConfigError):
            m.ready()


def test_mail_template_matches_spec() -> None:
    spec = Path(__file__).parents[2] / "spec" / "mail.html"
    here = Path(__file__).parents[1] / "src" / "serlog" / "mail.html"
    assert spec.read_text(encoding="utf-8") == here.read_text(encoding="utf-8")


def test_file_claims_its_canonical_path(tmp_path: Path) -> None:
    a = File(tmp_path / "x" / ".." / "same.log").claim()
    b = File(os.path.join(tmp_path, "same.log")).claim()
    assert a == b
