"""One log event as every sink sees it: immutable, so a processor that changes it makes a copy."""

import asyncio
import dataclasses
import itertools
import threading
from collections.abc import Iterable
from dataclasses import dataclass, field
from datetime import UTC, datetime

from .level import Level

__seq = itertools.count()


def now() -> datetime:
    """The current time, in UTC."""
    return datetime.now(UTC)


def next_seq() -> int:
    """The next record number."""
    return next(__seq)


def task() -> str | None:
    """The name of the running asyncio task, if any."""
    try:
        t = asyncio.current_task()
    except RuntimeError:
        return None
    return t.get_name() if t else None


@dataclass(frozen=True, slots=True, kw_only=True)
class Record:
    """A log event: level, target, message, typed fields, tags, caller, thread, span chain, time and number."""

    level: Level
    target: str
    msg: str
    fields: tuple[tuple[str, object], ...] = ()
    tags: tuple[str, ...] = ()
    module: str | None = None
    func: str | None = None
    file: str | None = None
    line: int | None = None
    thread: str | None = field(default_factory=lambda: threading.current_thread().name)
    task: str | None = field(default_factory=task)
    spans: str | None = None
    time: datetime = field(default_factory=now)
    seq: int = field(default_factory=next_seq)

    def field(self, key: str) -> object | None:
        """The last field called `key`."""
        return next((v for k, v in reversed(self.fields) if k == key), None)

    def replace(self, **changes: object) -> Record:
        """A copy with some parts changed."""
        return dataclasses.replace(self, **changes)

    def with_fields(self, pairs: Iterable[tuple[str, object]]) -> Record:
        """A copy with `pairs` set, replacing fields of the same name."""
        new = dict(pairs)
        kept = tuple((k, v) for k, v in self.fields if k not in new)
        return self.replace(fields=kept + tuple(new.items()))
