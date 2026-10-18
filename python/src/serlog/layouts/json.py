"""JSON lines in the shape other tools expect: serlog's own, pino, bunyan, zap, slog, structlog,
logstash, ECS, GELF or the OpenTelemetry log model; any key can be renamed."""

import os
from collections.abc import Mapping
from enum import StrEnum

from ..compat import Compat
from ..meta import host, meta
from ..record import Record
from . import clock
from .text import json_value, quote


class Schema(StrEnum):
    """A JSON shape."""

    SERLOG = "serlog"
    PINO = "pino"
    BUNYAN = "bunyan"
    ZAP = "zap"
    SLOG = "slog"
    STRUCTLOG = "structlog"
    LOGSTASH = "logstash"
    ECS = "ecs"
    GELF = "gelf"
    OTEL = "otel"


class Levels(StrEnum):
    """How levels are written."""

    NAME = "name"
    NUMBER = "number"


def logback_value(level: int) -> int:
    """Logback's level number."""
    return 5000 if level <= 10 else 10000 if level <= 20 else 20000 if level <= 30 else 30000 if level <= 40 else 40000


def otel_number(level: int) -> int:
    """The OpenTelemetry severity number."""
    return 1 if level <= 10 else 5 if level <= 20 else 9 if level <= 30 else 13 if level <= 40 else 17 if level <= 50 else 21


class Json:
    """A JSON layout: a schema, renamed keys, level style and where fields go."""

    __slots__ = ("__keys", "__levels", "__nest", "__schema")

    def __init__(
        self,
        schema: Schema | str = Schema.SERLOG,
        *,
        keys: Mapping[str, str] | None = None,
        levels: Levels | str | None = None,
        nest: str | None = None,
    ) -> None:
        self.__schema = Schema(schema)
        self.__keys = dict(keys or {})
        self.__levels = Levels(levels) if levels else None
        self.__nest = nest

    @property
    def schema(self) -> Schema:
        """The schema."""
        return self.__schema

    @property
    def keys(self) -> dict[str, str]:
        """The renamed keys."""
        return dict(self.__keys)

    @property
    def levels(self) -> Levels | None:
        """The level style, when set."""
        return self.__levels

    @property
    def nest(self) -> str | None:
        """The key fields are nested under, when set."""
        return self.__nest

    def __level(self, rec: Record, name: str, numeric: bool = False) -> str:
        style = self.__levels or (Levels.NUMBER if numeric else Levels.NAME)
        return str(int(rec.level)) if style is Levels.NUMBER else quote(name)

    def __fields(self, rec: Record, error: str) -> list[tuple[str, str]]:
        pairs = [(error if k == "error" else k, json_value(v)) for k, v in rec.fields]
        if self.__nest and pairs:
            return [(self.__nest, "{" + ",".join(f"{quote(self.__key(k))}:{v}" for k, v in pairs) + "}")]
        return pairs

    def __key(self, k: str) -> str:
        return self.__keys.get(k, k)

    def render(self, rec: Record) -> str:  # noqa: PLR0915
        """`rec` as one JSON object."""
        m = meta()
        q = quote
        ids = [(k, q(v)) for k, v in (("app", m.app), ("env", m.env), ("instance", m.instance)) if v]
        file_line = f"{rec.file}:{rec.line}" if rec.file and rec.line is not None else rec.file
        lvl = rec.level
        match self.__schema:
            case Schema.SERLOG:
                pairs = [("time", q(clock.iso(rec.time))), ("level", self.__level(rec, lvl.name)), ("target", q(rec.target)), ("msg", q(rec.msg))]
                pairs += [("file", q(rec.file))] if rec.file else []
                pairs += [("line", str(rec.line))] if rec.line is not None else []
                pairs += self.__fields(rec, "error") + ids
            case Schema.PINO:
                pairs = [
                    ("level", self.__level(rec, lvl.name, True)),
                    ("time", str(int(rec.time.timestamp() * 1000))),
                    ("pid", str(os.getpid())),
                    ("hostname", q(host())),
                ]
                pairs += [*self.__fields(rec, "err"), ("msg", q(rec.msg))]
            case Schema.BUNYAN:
                pairs = [("name", q(m.app)), ("hostname", q(host())), ("pid", str(os.getpid())), ("level", self.__level(rec, lvl.name, True))]
                pairs += [*self.__fields(rec, "err"), ("msg", q(rec.msg)), ("time", q(clock.iso(rec.time))), ("v", "0")]
            case Schema.ZAP:
                pairs = [("level", self.__level(rec, lvl.name)), ("ts", f"{rec.time.timestamp():.3f}"), ("logger", q(rec.target))]
                pairs += [("caller", q(file_line))] if file_line else []
                pairs += [("msg", q(rec.msg)), *self.__fields(rec, "error")]
            case Schema.SLOG:
                t = clock.strftime(rec.time.astimezone(), "%Y-%m-%dT%H:%M:%S%.9f%:z")
                pairs = [("time", q(t)), ("level", self.__level(rec, lvl.label(Compat.SLOG))), ("msg", q(rec.msg)), *self.__fields(rec, "error")]
            case Schema.STRUCTLOG:
                pairs = [("event", q(rec.msg)), *self.__fields(rec, "error"), ("level", self.__level(rec, lvl.name)), ("logger", q(rec.target))]
                pairs += [("timestamp", q(clock.iso(rec.time)))]
            case Schema.LOGSTASH:
                t = clock.strftime(rec.time.astimezone(), "%Y-%m-%dT%H:%M:%S%.3f%:z")
                pairs = [("@timestamp", q(t)), ("@version", q("1")), ("message", q(rec.msg)), ("logger_name", q(rec.target))]
                pairs += [("thread_name", q(rec.thread))] if rec.thread else []
                pairs += [("level", self.__level(rec, lvl.label(Compat.LOGBACK))), ("level_value", str(logback_value(lvl)))]
                pairs += self.__fields(rec, "stack_trace")
            case Schema.ECS:
                pairs = [("@timestamp", q(clock.iso(rec.time))), ("log.level", self.__level(rec, lvl.name)), ("message", q(rec.msg))]
                pairs += [("ecs.version", q("8.11.0")), ("log.logger", q(rec.target))]
                pairs += [("log.origin.file.name", q(rec.file))] if rec.file else []
                pairs += [("log.origin.file.line", str(rec.line))] if rec.line is not None else []
                pairs += [("process.pid", str(os.getpid())), ("host.hostname", q(host()))]
                svc = (("service.name", m.app), ("service.environment", m.env), ("service.node.name", m.instance))
                pairs += [(k, q(v)) for k, v in svc if v] + self.__fields(rec, "error")
            case Schema.GELF:
                pairs = [("version", q("1.1")), ("host", q(host())), ("short_message", q(rec.msg)), ("timestamp", f"{rec.time.timestamp():.3f}")]
                pairs += [("level", str(lvl.syslog)), ("_target", q(rec.target))]
                pairs += [(f"_{k}", json_value(v)) for k, v in rec.fields if k != "id"]
            case Schema.OTEL:
                attrs = [("code.namespace", q(rec.target))]
                attrs += [("code.filepath", q(rec.file))] if rec.file else []
                attrs += [("code.lineno", str(rec.line))] if rec.line is not None else []
                attrs += [("exception" if k == "error" else k, json_value(v)) for k, v in rec.fields]
                res = [(k, q(v)) for k, v in (("service.name", m.app), ("deployment.environment", m.env), ("service.instance.id", m.instance)) if v]
                pairs = [
                    ("timestamp", q(clock.iso(rec.time, 9))),
                    ("severity_text", q(lvl.label(Compat.SERLOG))),
                    ("severity_number", str(otel_number(lvl))),
                ]
                pairs += [("body", q(rec.msg)), ("attributes", self.__object(attrs)), ("resource", self.__object(res))]
        return self.__object(pairs)

    def __object(self, pairs: list[tuple[str, str]]) -> str:
        return "{" + ",".join(f"{quote(self.__key(k))}:{v}" for k, v in pairs) + "}"

    def __repr__(self) -> str:
        return f"Json({self.__schema.value!r})"
