"""Grafana Loki's push API, which Alloy's `loki.source.api` accepts as well."""

import gzip
import urllib.error
import urllib.request
from collections import defaultdict
from collections.abc import Iterable, Mapping, Sequence
from typing import Unpack, override

from ..errors import ConfigError, Rejected
from ..layouts import Layout, Pattern
from ..layouts.text import flat, quote, show
from ..meta import meta
from ..queues import Queue
from ..sink import Entry, Sink, SlotKeys
from ..units import seconds
from ..units import show as show_time


class Loki(Sink):
    """Pushes gzipped JSON, one stream per level; lines are the slot's layout, `{msg}` unless set."""

    kind = "loki"

    def __init__(
        self, url: str, *, labels: Mapping[str, str] | None = None, timeout: float | str = 10, connect: bool = False, **slot: Unpack[SlotKeys]
    ) -> None:
        super().__init__(**slot)
        self.__url = url
        self.__labels = dict(labels or {})
        self.__timeout = seconds(timeout)
        self.__connect = connect

    @staticmethod
    def __object(pairs: Iterable[tuple[str, str]]) -> str:
        return "{" + ",".join(f"{quote(k)}:{quote(v)}" for k, v in pairs) + "}"

    def body(self, entries: Sequence[Entry]) -> str:
        """The push request body: labels per stream, structured metadata per line."""
        streams: dict[int, list[Entry]] = defaultdict(list)
        for e in entries:
            streams[e.record.level].append(e)
        m, obj = meta(), self.__object
        out = []
        for level in sorted(streams):
            name = streams[level][0].record.level.name
            base = [(k, v) for k, v in (("app", m.app), ("env", m.env), ("instance", m.instance), ("level", name)) if v]
            values = []
            for e in streams[level]:
                r = e.record
                ns = int(r.time.timestamp()) * 10**9 + r.time.microsecond * 1000
                extra = [("target", r.target)]
                if r.file and r.line is not None:
                    extra.append(("file", f"{r.file}:{r.line}"))
                extra += [(k, show(v)) for k, v in flat(r.fields)]
                values.append(f'["{ns}",{quote(e.line)},{obj(extra)}]')
            out.append(f'{{"stream":{obj([*base, *self.__labels.items()])},"values":[{",".join(values)}]}}')
        return '{"streams":[' + ",".join(out) + "]}"

    @override
    def emit(self, entry: Entry) -> None:
        self.emit_batch([entry])

    @override
    def emit_batch(self, entries: Sequence[Entry]) -> None:
        if not entries:
            return
        data = gzip.compress(self.body(entries).encode())
        req = urllib.request.Request(self.__url, data, {"Content-Type": "application/json", "Content-Encoding": "gzip"})
        try:
            with urllib.request.urlopen(req, timeout=self.__timeout):
                return
        except urllib.error.HTTPError as e:
            if 400 <= e.code < 500 and e.code != 429:
                raise Rejected(f"loki answered {e.code}") from e
            raise OSError(f"loki answered {e.code}") from e

    def __base(self) -> str:
        i = self.__url.find("/loki/api/")
        return self.__url[:i] if i >= 0 else self.__url.rstrip("/")

    @override
    def ready(self) -> None:
        if not self.__url.startswith(("http://", "https://")):
            raise ConfigError(f"loki url {self.__url!r} is not http or https")
        if self.__connect:
            with urllib.request.urlopen(f"{self.__base()}/ready", timeout=self.__timeout):
                pass

    @override
    def default_layout(self) -> Layout:
        return Pattern("{msg}")

    @override
    def queued(self) -> Queue:
        return Queue(8192)

    @override
    def options(self) -> dict[str, object]:
        out: dict[str, object] = {"url": self.__url}
        if self.__labels:
            out["labels"] = dict(self.__labels)
        if self.__timeout != 10:
            out["timeout"] = show_time(self.__timeout)
        if self.__connect:
            out["connect"] = True
        return out
