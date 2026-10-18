"""Line templates: `{time} {level:<5} {target}: {msg}{fields}`, with width, alignment and truncation on any part."""

import os
import re
import threading
from dataclasses import dataclass

from ..colour import Theme, paint
from ..compat import PATTERNS, Compat, compat
from ..meta import host, meta
from ..record import Record
from . import clock
from .text import fields, logfmt, show, stack

__FIT = re.compile(r"(?:([<>^])(\d+))?(?:\.(\d+))?")
__PLAIN = frozenset({"module", "func", "line", "thread", "tid", "task", "pid", "host", "app", "env", "instance", "tags", "elapsed", "seq", "spans"})
__OPTIONS = {"level": "lower", "file": "base", "msg": "q", "fields": "q", "error": "stack"}


@dataclass(frozen=True, slots=True)
class Piece:
    """One literal or one token with its option and fit."""

    kind: str
    option: object = None
    align: str | None = None
    width: int = 0
    cut: int | None = None


def shorten(target: str, n: int) -> str:
    """Shortens leading segments of `target` to their first letter until it fits in `n`."""
    if len(target) <= n:
        return target
    sep = "::" if "::" in target else "."
    parts = target.split(sep)
    out = list(parts)
    for i in range(len(parts) - 1):
        if len(sep.join(out)) <= n:
            break
        out[i] = parts[i][:1]
    return sep.join(out)


def piece(name: str, spec: str, source: str) -> Piece:
    """Parses one `{name:spec}` hole."""

    def bad(why: str) -> ValueError:
        return ValueError(f"pattern {source!r}: {why}")

    if name == "time":
        return Piece("time", spec)
    option: object = None
    rest = spec
    if name == "target" and spec.startswith("~"):
        m = re.match(r"~(\d+)", spec)
        if not m:
            raise bad(f"bad `~` in `{spec}`")
        option, rest = int(m[1]), spec[m.end() :]
    elif name in __OPTIONS:
        word = __OPTIONS[name]
        option, rest = (True, spec[len(word) :]) if spec.startswith(word) else (False, spec)
    elif name not in __PLAIN and name != "target":
        raise bad(f"unknown token `{name}`")
    m = __FIT.fullmatch(rest)
    if not m:
        raise bad(f"bad spec `{spec}` for `{name}`")
    return Piece(name, option, m[1], int(m[2] or 0), int(m[3]) if m[3] else None)


class Pattern:
    """A parsed line template: `{token}` or `{token:spec}` holes; `{{` and `}}` are literal braces.

    Every token but `time` may end with `<N`, `>N` or `^N` to pad, then `.M` to cut. A preset names its
    levels the way its logger does, such as `WARNING` for `python`; other patterns follow the profile in force.
    """

    __slots__ = ("__labels", "__pieces", "__shows_error", "__source")
    __HOLE = re.compile(r"\{\{|\}\}|\{([^}]*)\}|\}")

    def __init__(self, source: str) -> None:
        """Parses a pattern, or a preset name such as `tracing`."""
        named = "logback" if source == "log4j" else source
        s = PATTERNS.get(named, source)
        self.__source = source
        self.__labels = Compat(named) if named in PATTERNS else None
        pieces: list[Piece] = []
        at = 0
        lit = ""
        for m in self.__HOLE.finditer(s):
            lit += s[at : m.start()]
            match m[0]:
                case "{{" | "}}":
                    lit += m[0][0]
                case "}":
                    raise ValueError(f"pattern {s!r}: stray `}}`")
                case _:
                    if lit:
                        pieces.append(Piece("", lit))
                        lit = ""
                    name, _, spec = m[1].partition(":")
                    pieces.append(piece(name.strip(), spec, s))
            at = m.end()
        if "{" in s[at:]:
            raise ValueError(f"pattern {s!r}: unclosed `{{`")
        lit += s[at:]
        if lit:
            pieces.append(Piece("", lit))
        self.__pieces = tuple(pieces)
        self.__shows_error = any(p.kind == "error" for p in pieces)

    @property
    def source(self) -> str:
        """The text it was parsed from, a preset name included."""
        return self.__source

    def render(self, rec: Record, theme: Theme | None = None) -> str:
        """`rec` as one line; a theme paints the level."""
        out = []
        for p in self.__pieces:
            if not p.kind:
                out.append(str(p.option))
                continue
            text = self.__token(p, rec)
            if p.cut is not None:
                text = text[: p.cut]
            if p.align:
                text = format(text, f"{p.align}{p.width}")
            if p.kind == "level" and theme is not None:
                text = paint(True, theme.of(rec.level), text)
            out.append(text)
        return "".join(out)

    def __token(self, p: Piece, rec: Record) -> str:
        match p.kind:
            case "time":
                return clock.write(str(p.option), rec.time)
            case "level":
                return rec.level.name if p.option else rec.level.label(self.__labels or compat())
            case "target":
                return shorten(rec.target, p.option) if isinstance(p.option, int) and not isinstance(p.option, bool) else rec.target
            case "module":
                return rec.module or rec.target
            case "func":
                return rec.func or "?"
            case "file":
                f = rec.file or "?"
                return os.path.basename(f) if p.option else f
            case "line":
                return "?" if rec.line is None else str(rec.line)
            case "thread":
                return rec.thread or "?"
            case "tid":
                return str(threading.get_native_id())
            case "task":
                return rec.task or "?"
            case "pid":
                return str(os.getpid())
            case "host":
                return host()
            case "app" | "env" | "instance":
                return getattr(meta(), p.kind)
            case "msg":
                return logfmt(rec.msg) if p.option else rec.msg
            case "fields":
                return fields(rec, bool(p.option), ("error",) if self.__shows_error else ())
            case "tags":
                return ",".join(rec.tags)
            case "error":
                e = rec.field("error")
                if e is None:
                    return ""
                trace = stack(e) if p.option and isinstance(e, BaseException) else None
                return show(e) + (f"\n{trace}" if trace else "")
            case "elapsed":
                return str(clock.elapsed())
            case "seq":
                return str(rec.seq)
            case "spans":
                return f"{rec.spans}: " if rec.spans else ""
        return ""

    def __str__(self) -> str:
        return self.__source

    def __repr__(self) -> str:
        return f"Pattern({self.__source!r})"
