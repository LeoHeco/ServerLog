"""The built-in steps: redact, sanitise, fields, match, exclude, sample, limit and dedupe."""

import re
import threading
import time
from abc import ABC, abstractmethod
from collections.abc import Iterable, Mapping
from typing import ClassVar, override

from ..level import Level, Levelish
from ..record import Record
from ..units import seconds, show


class Processor(ABC):
    """One step of a chain; built-in steps can also describe themselves for export."""

    kind: ClassVar[str] = "custom"

    @abstractmethod
    def __call__(self, rec: Record) -> Record | None:
        """The record, a changed copy, or `None` to drop it."""

    def options(self) -> dict[str, object] | None:
        """The config entry that rebuilds this step, or `None` when it cannot be exported."""
        return None


class Redact(Processor):
    """Masks fields by name at any depth or by dotted path, and parts of the message by regex."""

    kind = "redact"

    def __init__(self, keys: Iterable[str] = (), *, paths: Iterable[str] = (), patterns: Iterable[str] = (), mask: str = "***") -> None:
        self.__keys = frozenset(keys)
        self.__paths = tuple(tuple(p.split(".")) for p in paths)
        self.__patterns = tuple(re.compile(p) for p in patterns)
        self.__mask = mask

    def __hit(self, path: tuple[str, ...]) -> bool:
        return path[-1] in self.__keys or path in self.__paths

    def __scrub(self, path: tuple[str, ...], v: object) -> object:
        if self.__hit(path):
            return self.__mask
        if isinstance(v, Mapping):
            return {k: self.__scrub((*path, str(k)), x) for k, x in v.items()}
        return v

    @override
    def __call__(self, rec: Record) -> Record:
        fields = tuple((k, self.__scrub((k,), v)) for k, v in rec.fields)
        msg = rec.msg
        for p in self.__patterns:
            msg = p.sub(self.__mask, msg)
        if msg == rec.msg and all(a[1] is b[1] for a, b in zip(fields, rec.fields, strict=True)):
            return rec
        return rec.replace(fields=fields, msg=msg)

    @override
    def options(self) -> dict[str, object]:
        out: dict[str, object] = {}
        if self.__keys:
            out["keys"] = sorted(self.__keys)
        if self.__paths:
            out["paths"] = [".".join(p) for p in self.__paths]
        if self.__patterns:
            out["patterns"] = [p.pattern for p in self.__patterns]
        if self.__mask != "***":
            out["mask"] = self.__mask
        return {"redact": out}


class Sanitise(Processor):
    """Escapes line breaks and control characters in the message and string fields, against log forging."""

    kind = "sanitise"
    __CONTROL = re.compile(r"[\x00-\x1f\x7f]")

    @classmethod
    def __clean(cls, s: str) -> str:
        names = {"\n": "\\n", "\r": "\\r", "\t": "\\t"}
        return cls.__CONTROL.sub(lambda m: names.get(m[0]) or f"\\u{{{ord(m[0]):04x}}}", s)

    @override
    def __call__(self, rec: Record) -> Record:
        msg = self.__clean(rec.msg)
        fields = tuple((k, self.__clean(v) if isinstance(v, str) else v) for k, v in rec.fields)
        if msg == rec.msg and fields == rec.fields:
            return rec
        return rec.replace(msg=msg, fields=fields)

    @override
    def options(self) -> dict[str, object]:
        return {"sanitise": True}


class Fields(Processor):
    """Sets the same fields on every record."""

    kind = "fields"

    def __init__(self, **fields: object) -> None:
        self.__fields = fields

    @override
    def __call__(self, rec: Record) -> Record:
        return rec.with_fields(self.__fields.items()) if self.__fields else rec

    @override
    def options(self) -> dict[str, object]:
        return {"fields": dict(self.__fields)}


class Match(Processor):
    """Keeps a record only when every condition given holds: tags, fields shown as text, a lowest level."""

    kind = "match"

    def __init__(self, *, tags: Iterable[str] = (), fields: Mapping[str, object] | None = None, level: Levelish | None = None) -> None:
        self.__tags = tuple(tags)
        self.__fields = {k: str(v) for k, v in (fields or {}).items()}
        self.__level = None if level is None else Level(level)

    def holds(self, rec: Record) -> bool:
        """Whether every condition holds."""
        from ..layouts.text import show as text

        return (
            (self.__level is None or rec.level >= self.__level)
            and all(t in rec.tags for t in self.__tags)
            and all(rec.field(k) is not None and text(rec.field(k)) == v for k, v in self.__fields.items())
        )

    @override
    def __call__(self, rec: Record) -> Record | None:
        return rec if self.holds(rec) else None

    def conditions(self) -> dict[str, object]:
        """The conditions as config keys."""
        out: dict[str, object] = {}
        if self.__tags:
            out["tags"] = list(self.__tags)
        if self.__fields:
            out["fields"] = dict(self.__fields)
        if self.__level is not None:
            out["level"] = self.__level.name
        return out

    @override
    def options(self) -> dict[str, object]:
        return {self.kind: self.conditions()}


class Exclude(Match):
    """Drops a record when every condition holds."""

    kind = "exclude"

    @override
    def __call__(self, rec: Record) -> Record | None:
        return None if self.holds(rec) else rec


class Sample(Processor):
    """Per level and message, keeps the first `first` in each `per`, then one in `every`."""

    kind = "sample"

    def __init__(self, first: int = 100, every: int = 100, per: float | str = 1.0) -> None:
        self.__first, self.__every, self.__per = first, max(every, 1), seconds(per)
        self.__seen: dict[tuple[int, str], list[float]] = {}
        self.__lock = threading.Lock()

    @override
    def __call__(self, rec: Record) -> Record | None:
        now = time.monotonic()
        with self.__lock:
            if len(self.__seen) > 10_000:
                self.__seen = {k: v for k, v in self.__seen.items() if now - v[0] < self.__per}
            slot = self.__seen.setdefault((int(rec.level), rec.msg), [now, 0])
            if now - slot[0] >= self.__per:
                slot[:] = [now, 0]
            slot[1] += 1
            n = int(slot[1])
        return rec if n <= self.__first or (n - self.__first) % self.__every == 0 else None

    @override
    def options(self) -> dict[str, object]:
        return {"sample": {"first": self.__first, "every": self.__every, "per": show(self.__per)}}


class Limit(Processor):
    """A token bucket: `rate` records a second, up to `burst` at once."""

    kind = "limit"

    def __init__(self, rate: float, burst: float | None = None) -> None:
        self.__rate = rate
        self.__burst = max(burst if burst is not None else rate, 1.0)
        self.__tokens = self.__burst
        self.__last = time.monotonic()
        self.__lock = threading.Lock()

    @override
    def __call__(self, rec: Record) -> Record | None:
        with self.__lock:
            now = time.monotonic()
            self.__tokens = min(self.__burst, self.__tokens + (now - self.__last) * self.__rate)
            self.__last = now
            if self.__tokens < 1:
                return None
            self.__tokens -= 1
        return rec

    @override
    def options(self) -> dict[str, object]:
        return {"limit": {"rate": self.__rate, "burst": self.__burst}}


class Dedupe(Processor):
    """Drops a record repeated within `within`; the next one kept carries `repeated`."""

    kind = "dedupe"

    def __init__(self, within: float | str = 1.0) -> None:
        self.__within = seconds(within)
        self.__seen: dict[tuple[int, str, str], list[float]] = {}
        self.__lock = threading.Lock()

    @override
    def __call__(self, rec: Record) -> Record | None:
        key = (int(rec.level), rec.target, rec.msg)
        now = time.monotonic()
        with self.__lock:
            if len(self.__seen) > 10_000:
                self.__seen = {k: v for k, v in self.__seen.items() if now - v[0] < self.__within}
            slot = self.__seen.get(key)
            if slot and now - slot[0] < self.__within:
                slot[1] += 1
                return None
            repeated = int(slot[1]) if slot else 0
            self.__seen[key] = [now, 0]
        return rec.with_fields([("repeated", repeated)]) if repeated else rec

    @override
    def options(self) -> dict[str, object]:
        return {"dedupe": {"within": show(self.__within)}}
