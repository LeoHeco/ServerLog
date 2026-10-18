"""A log file, rolled over by size or time, cleaned up by count, age or total size, and reopened when moved."""

import fnmatch
import gzip as gz
import os
import shutil
import threading
import time
from collections.abc import Sequence
from datetime import datetime
from enum import StrEnum
from pathlib import Path
from typing import IO, Unpack, override

from ..errors import ConfigError
from ..level import Level
from ..sink import Entry, Sink, SlotKeys
from ..units import seconds, show, show_size, size

NAME = "{stem}.{time:%Y%m%d-%H%M%S}.{ext}"
"""The default name of a rolled file."""


class Rotate(StrEnum):
    """When files roll over by time, on local time."""

    MINUTE = "minute"
    HOUR = "hour"
    DAY = "day"
    WEEK = "week"

    def period(self, t: datetime) -> str:
        """A key that changes exactly when a new period starts."""
        local = t.astimezone()
        return local.strftime({"minute": "%Y%m%d%H%M", "hour": "%Y%m%d%H", "day": "%Y%m%d", "week": "%G%V"}[self.value])


class File(Sink):
    """Appends one line per entry to `path`."""

    kind = "file"

    def __init__(
        self,
        path: str | os.PathLike[str],
        *,
        append: bool = True,
        perms: str | int | None = None,
        mkdir: bool = True,
        max_size: int | str | None = None,
        rotate: Rotate | str | None = None,
        keep: int | None = None,
        max_age: float | str | None = None,
        max_total: int | str | None = None,
        gzip: bool = False,
        roll_name: str | None = None,
        reopen: bool = False,
        buffer: int | str = 0,
        flush_every: float | str = 1.0,
        **slot: Unpack[SlotKeys],
    ) -> None:
        super().__init__(**slot)
        self.__path = Path(path)
        self.__append, self.__mkdir, self.__gzip, self.__reopen = append, mkdir, gzip, reopen
        self.__perms = int(perms, 8) if isinstance(perms, str) else perms
        self.__max = None if max_size is None else size(max_size)
        self.__rotate = Rotate(rotate) if rotate else None
        self.__keep = keep
        self.__age = None if max_age is None else seconds(max_age)
        self.__total = None if max_total is None else size(max_total)
        self.__template = roll_name or NAME
        self.__buffer = size(buffer)
        self.__every = seconds(flush_every)
        self.__lock = threading.Lock()
        self.__out: IO[str] | None = None
        self.__size = 0
        self.__period: str | None = None
        self.__began = datetime.now().astimezone()
        self.__checked = 0.0
        self.__flushed: float | None = None
        self.__jobs: list[threading.Thread] = []

    @property
    def path(self) -> Path:
        """The path written to."""
        return self.__path

    def __open(self, fresh: bool) -> None:
        mode = "w" if fresh and not self.__append else "a"
        new = not self.__path.exists()
        self.__out = self.__path.open(mode, encoding="utf-8", buffering=self.__buffer or 1, newline="\n")
        if new and self.__perms is not None:
            os.chmod(self.__path, self.__perms)
        st = self.__path.stat()
        self.__size = st.st_size
        self.__began = datetime.fromtimestamp(st.st_mtime).astimezone()
        self.__period = self.__rotate.period(self.__began) if self.__rotate else None

    def __moved(self) -> bool:
        try:
            return self.__path.stat().st_size < self.__size
        except FileNotFoundError:
            return True

    def __name(self, began: datetime) -> Path:
        stem, ext = self.__path.stem, self.__path.suffix.lstrip(".")
        out, rest = "", self.__template
        while "{" in rest:
            i = rest.index("{")
            out += rest[:i]
            end = rest.find("}", i)
            if end < 0:
                break
            hole = rest[i + 1 : end]
            out += stem if hole == "stem" else ext if hole == "ext" else began.strftime(hole[5:]) if hole.startswith("time:") else rest[i : end + 1]
            rest = rest[end + 1 :]
        out = (out + rest).rstrip(".")
        first = self.__path.with_name(out)

        def free(p: Path) -> bool:
            return not p.exists() and not p.with_name(p.name + ".gz").exists()

        if free(first):
            return first
        base, dot, tail = out.rpartition(".")
        base, tail = (base, f".{tail}") if dot else (out, "")
        return next(p for n in range(1, 1 << 30) if free(p := self.__path.with_name(f"{base}-{n}{tail}")))

    def __shape(self) -> tuple[str, str]:
        stem, ext = self.__path.stem, self.__path.suffix.lstrip(".")
        plain, rest = "", self.__template
        while "{" in rest:
            i = rest.index("{")
            end = rest.find("}", i)
            hole = rest[i + 1 : end]
            plain += rest[:i] + (stem if hole == "stem" else ext if hole == "ext" else "*")
            rest = rest[end + 1 :]
        plain = (plain + rest).rstrip(".")
        base, dot, tail = plain.rpartition(".")
        return plain, f"{base}*.{tail}" if dot else plain + "*"

    def __prune(self) -> None:
        if self.__keep is None and self.__age is None and self.__total is None:
            return
        plain, numbered = self.__shape()
        old = []
        for p in self.__path.parent.iterdir():
            name = p.name.removesuffix(".gz")
            if p != self.__path and (fnmatch.fnmatchcase(name, plain) or fnmatch.fnmatchcase(name, numbered)):
                st = p.stat()
                old.append((st.st_mtime, str(p), st.st_size))
        old.sort()
        now, total, left = time.time(), sum(n for *_, n in old), len(old)
        for mtime, p, n in old:
            aged = self.__age is not None and now - mtime > self.__age
            many = self.__keep is not None and left > self.__keep
            big = self.__total is not None and total > self.__total
            if aged or many or big:
                Path(p).unlink(missing_ok=True)
                left, total = left - 1, total - n

    def __after(self, rolled: Path) -> None:
        if self.__gzip:
            target = rolled.with_name(rolled.name + ".gz")
            with rolled.open("rb") as src, gz.open(target, "wb") as dst:
                shutil.copyfileobj(src, dst)
            mtime = rolled.stat().st_mtime
            os.utime(target, (mtime, mtime))
            rolled.unlink()
        self.__prune()

    def __roll(self, t: datetime) -> None:
        if self.__out:
            self.__out.close()
        to = self.__name(self.__began)
        self.__path.rename(to)
        self.__open(False)
        self.__began = t.astimezone()
        self.__period = self.__rotate.period(t) if self.__rotate else None
        for j in self.__jobs:
            j.join()
        job = threading.Thread(target=self.__after, args=(to,), name="serlog-roll", daemon=True)
        job.start()
        self.__jobs = [job]

    def __put(self, text: str, t: datetime, urgent: bool) -> None:
        with self.__lock:
            now = time.monotonic()
            if self.__reopen and self.__out is not None and now - self.__checked >= 1:
                self.__checked = now
                if self.__moved():
                    self.__out.close()
                    self.__out = None
            if self.__out is None:
                self.__open(False)
            n = len(text.encode())
            full = self.__max is not None and self.__size > 0 and self.__size + n > self.__max
            turned = self.__rotate is not None and self.__period != self.__rotate.period(t)
            if full or turned:
                self.__roll(t)
            assert self.__out is not None
            self.__out.write(text)
            if self.__buffer and (urgent or self.__flushed is None or now - self.__flushed >= self.__every):
                self.__out.flush()
                self.__flushed = now
            self.__size += n

    @override
    def emit(self, entry: Entry) -> None:
        r = entry.record
        self.__put(entry.line + "\n", r.time, r.level >= Level.ERROR)

    @override
    def emit_batch(self, entries: Sequence[Entry]) -> None:
        if not entries:
            return
        if self.__max is not None or self.__rotate is not None:
            for e in entries:
                self.emit(e)
            return
        urgent = any(e.record.level >= Level.ERROR for e in entries)
        self.__put("".join(e.line + "\n" for e in entries), entries[-1].record.time, urgent)

    @override
    def flush(self) -> None:
        with self.__lock:
            if self.__out:
                self.__out.flush()
            jobs, self.__jobs = self.__jobs, []
        for j in jobs:
            j.join()

    @override
    def close(self) -> None:
        self.flush()
        with self.__lock:
            if self.__out:
                self.__out.close()
                self.__out = None

    @override
    def ready(self) -> None:
        if self.__path.is_dir():
            raise ConfigError(f"{self.__path} is a directory")
        parent = self.__path.parent
        if self.__mkdir:
            parent.mkdir(parents=True, exist_ok=True)
        elif not parent.is_dir():
            raise ConfigError(f"{parent} does not exist")
        with self.__lock:
            self.__open(True)

    @override
    def claim(self) -> str:
        p = os.path.normcase(os.path.abspath(os.path.realpath(self.__path)))
        return f"file {p}"

    @override
    def options(self) -> dict[str, object]:
        out: dict[str, object] = {"path": str(self.__path)}
        if not self.__append:
            out["append"] = False
        if self.__perms is not None:
            out["perms"] = f"{self.__perms:04o}"
        if not self.__mkdir:
            out["mkdir"] = False
        if self.__max is not None:
            out["max_size"] = show_size(self.__max)
        if self.__rotate:
            out["rotate"] = self.__rotate.value
        if self.__keep is not None:
            out["keep"] = self.__keep
        if self.__age is not None:
            out["max_age"] = show(self.__age)
        if self.__total is not None:
            out["max_total"] = show_size(self.__total)
        if self.__gzip:
            out["gzip"] = True
        if self.__template != NAME:
            out["roll_name"] = self.__template
        if self.__reopen:
            out["reopen"] = True
        if self.__buffer:
            out["buffer"] = show_size(self.__buffer)
            out["flush_every"] = show(self.__every)
        return out
