"""How a record becomes text: a pattern, logfmt, a JSON schema, the pretty layout, or a function."""

from collections.abc import Callable, Mapping
from functools import cache

from ..colour import Theme
from ..compat import PATTERNS, Compat, compat
from ..errors import ConfigError
from ..record import Record
from .json import Json, Levels, Schema
from .pattern import Pattern, shorten
from .text import json_value, logfmt, quote, render_logfmt, render_pretty, show


class Inherit:
    """The pattern of the compat profile in force."""

    __slots__ = ()

    @staticmethod
    @cache
    def __profile(p: Compat) -> Pattern:
        return Pattern(p.value)

    def render(self, rec: Record, theme: Theme | None = None) -> str:
        """`rec` through the profile's pattern."""
        return self.__profile(compat()).render(rec, theme)


class Logfmt:
    """`time=… level=… target=… msg=… key=value…`"""

    __slots__ = ()

    def render(self, rec: Record, theme: Theme | None = None) -> str:
        """`rec` as logfmt."""
        return render_logfmt(rec)


class Pretty:
    """A first line plus one line per field, for reading in a terminal."""

    __slots__ = ()

    def render(self, rec: Record, theme: Theme | None = None) -> str:
        """`rec` over several lines."""
        return render_pretty(rec, theme)


class Custom:
    """A layout written by the caller: a function from a record to a line."""

    __slots__ = ("__fn",)

    def __init__(self, fn: Callable[[Record], str]) -> None:
        self.__fn = fn

    def render(self, rec: Record, theme: Theme | None = None) -> str:
        """`rec` as the function lays it out."""
        return self.__fn(rec)


class JsonLayout:
    """A [`Json`] schema as a layout."""

    __slots__ = ("json",)

    def __init__(self, json: Json) -> None:
        self.json = json

    def render(self, rec: Record, theme: Theme | None = None) -> str:
        """`rec` as one JSON object."""
        return self.json.render(rec)


type Layout = Inherit | Pattern | Logfmt | JsonLayout | Pretty | Custom
"""How a record becomes one line."""

INHERIT = Inherit()
LOGFMT = Logfmt()
PRETTY = Pretty()


def as_layout(x: Layout | Json | Schema | str | Callable[[Record], str]) -> Layout:
    """A layout from a pattern or preset name, a schema, a function, or a layout."""
    match x:
        case Inherit() | Pattern() | Logfmt() | JsonLayout() | Pretty() | Custom():
            return x
        case Json():
            return JsonLayout(x)
        case Schema():
            return JsonLayout(Json(x))
        case str():
            return Pattern(x)
    return Custom(x)


def build(
    *,
    layout: str | None = None,
    format: str | None = None,
    pattern: str | None = None,
    schema: str | None = None,
    keys: Mapping[str, str] | None = None,
    levels: str | None = None,
    nest: str | None = None,
) -> Layout | None:
    """The layout a slot's keys ask for, or `None` for the sink's default."""
    if layout is not None:
        from ..config.kinds import layout_maker

        return layout_maker(layout)()
    jsonish = schema is not None or bool(keys) or levels is not None or nest is not None
    match format, pattern:
        case "json", None:
            return JsonLayout(Json(schema or Schema.SERLOG, keys=keys, levels=levels, nest=nest))
        case None, None if jsonish:
            return JsonLayout(Json(schema or Schema.SERLOG, keys=keys, levels=levels, nest=nest))
        case "logfmt", None:
            return LOGFMT
        case "pretty", None:
            return PRETTY
        case None | "text", str() as p:
            return Pattern(p)
        case None | "text", None:
            return None
        case f, str():
            raise ConfigError(f"`pattern` goes with `format: text`, not {f!r}")
    raise ConfigError(f"`format` takes text, logfmt, json or pretty, not {format!r}")


__all__ = [
    "INHERIT",
    "LOGFMT",
    "PATTERNS",
    "PRETTY",
    "Custom",
    "Inherit",
    "Json",
    "JsonLayout",
    "Layout",
    "Levels",
    "Logfmt",
    "Pattern",
    "Pretty",
    "Schema",
    "as_layout",
    "build",
    "json_value",
    "logfmt",
    "quote",
    "shorten",
    "show",
]
