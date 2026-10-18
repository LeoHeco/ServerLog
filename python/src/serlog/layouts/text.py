"""Text pieces shared by the layouts: quoting, values, field lists, logfmt and the pretty layout."""

import math
import traceback
from collections.abc import Iterator, Mapping, Sequence

from ..colour import Colour, Theme, paint
from ..compat import compat
from ..record import Record
from .clock import iso, write

__ESCAPES = {'"': '\\"', "\\": "\\\\", "\n": "\\n", "\r": "\\r", "\t": "\\t"}


def quote(v: object) -> str:
    """`v` as a JSON string, escaped as every serlog package escapes it."""
    out = (__ESCAPES.get(c) or (f"\\u{ord(c):04x}" if c < " " else c) for c in str(v))
    return '"' + "".join(out) + '"'


def logfmt(v: object) -> str:
    """`v` bare, or quoted when it is empty or holds whitespace, quotes or `=`."""
    s = show(v)
    return s if s and not any(c.isspace() or c in '"=' for c in s) else quote(s)


def causes(e: BaseException) -> Iterator[BaseException]:
    """What `e` was raised from, then what that was raised from, and so on."""
    seen = {id(e)}
    cur = e.__cause__ or (None if e.__suppress_context__ else e.__context__)
    while cur is not None and id(cur) not in seen:
        seen.add(id(cur))
        yield cur
        cur = cur.__cause__ or (None if cur.__suppress_context__ else cur.__context__)


def stack(e: BaseException) -> str | None:
    """The traceback, when `e` was raised."""
    return "".join(traceback.format_exception(e)).rstrip() if e.__traceback__ else None


def show(v: object) -> str:
    """A value as text: `null`, `true`, `[a, b]`, `{k=v}`, errors with their causes."""
    match v:
        case None:
            return "null"
        case bool():
            return "true" if v else "false"
        case str():
            return v
        case BaseException():
            return ": ".join(str(x) for x in (v, *causes(v)))
        case Mapping():
            return "{" + ", ".join(f"{k}={show(x)}" for k, x in v.items()) + "}"
        case list() | tuple() | set() | frozenset():
            return "[" + ", ".join(show(x) for x in v) + "]"
    return str(v)


def json_value(v: object) -> str:
    """A value as JSON."""
    match v:
        case None:
            return "null"
        case bool():
            return "true" if v else "false"
        case int():
            return str(v)
        case float():
            return repr(v) if math.isfinite(v) else quote(v)
        case str():
            return quote(v)
        case BaseException():
            parts = [f'"message":{quote(v)}']
            if c := list(causes(v)):
                parts.append('"causes":[' + ",".join(quote(x) for x in c) + "]")
            if s := stack(v):
                parts.append(f'"stack":{quote(s)}')
            return "{" + ",".join(parts) + "}"
        case Mapping():
            return "{" + ",".join(f"{quote(k)}:{json_value(x)}" for k, x in v.items()) + "}"
        case list() | tuple() | set() | frozenset():
            return "[" + ",".join(json_value(x) for x in v) + "]"
    return quote(v)


def flat(fields: Sequence[tuple[str, object]], skip: tuple[str, ...] = ()) -> Iterator[tuple[str, object]]:
    """Each field, maps spread into `key.sub`."""

    def walk(key: str, v: object) -> Iterator[tuple[str, object]]:
        if isinstance(v, Mapping):
            for k, x in v.items():
                yield from walk(f"{key}.{k}", x)
        else:
            yield key, v

    for k, v in fields:
        if k not in skip:
            yield from walk(k, v)


def fields(rec: Record, quoted: bool, skip: tuple[str, ...] = ()) -> str:
    """` key=value` per field."""
    put = logfmt if quoted else show
    return "".join(f" {k}={put(v)}" for k, v in flat(rec.fields, skip))


def render_logfmt(rec: Record) -> str:
    """`time=… level=… target=… msg=… key=value…`"""
    return f"time={iso(rec.time, 0)} level={rec.level.name} target={logfmt(rec.target)} msg={logfmt(rec.msg)}{fields(rec, True)}"


def render_pretty(rec: Record, theme: Theme | None) -> str:
    """A first line with time, level, target and message, then one aligned line per field."""
    on = theme is not None
    level = paint(on, theme.of(rec.level) if theme else Colour.PLAIN, f"{rec.level.label(compat()):<5}")
    head = f"{paint(on, Colour.GREY, write('', rec.time))} {level} {paint(on, Colour.WHITE, rec.target)} {rec.msg}"
    rows = [(k, show(v)) for k, v in flat(rec.fields)]
    width = max((len(k) for k, _ in rows), default=0)
    return head + "".join(f"\n    {paint(on, Colour.CYAN, k.ljust(width))} = {v}" for k, v in rows)
