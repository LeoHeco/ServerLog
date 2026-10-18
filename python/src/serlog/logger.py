"""Making records: loggers with a target and fields, the module-level calls, and catching exceptions."""

import functools
import inspect
import sys
from collections.abc import Callable
from contextlib import ContextDecorator
from string.templatelib import Interpolation, Template
from types import TracebackType
from typing import Self

from . import hub
from .level import Level, Levelish
from .record import Record


class Lazy:
    """A field or argument worked out only when the record passes: `serlog.debug("{}", serlog.lazy(dump))`."""

    __slots__ = ("fn",)

    def __init__(self, fn: Callable[[], object]) -> None:
        self.fn = fn


def lazy(fn: Callable[[], object]) -> Lazy:
    """Defers `fn` until a record is made."""
    return Lazy(fn)


def __value(v: object) -> object:
    return v.fn() if isinstance(v, Lazy) else v


def __render(msg: str | Template, args: tuple[object, ...]) -> tuple[str, tuple[tuple[str, object], ...]]:
    if isinstance(msg, Template):
        parts, found = [], []
        for item in msg:
            if isinstance(item, Interpolation):
                v = __value(item.value)
                shown = repr(v) if item.conversion == "r" else ascii(v) if item.conversion == "a" else v
                parts.append(format(shown, item.format_spec))
                if item.expression.isidentifier():
                    found.append((item.expression, v))
            else:
                parts.append(item)
        return "".join(parts), tuple(found)
    if args:
        return str(msg).format(*(__value(a) for a in args)), ()
    return str(msg), ()


def make(
    level: Level,
    target: str | None,
    msg: str | Template,
    args: tuple[object, ...],
    fields: dict[str, object],
    *,
    bound: tuple[tuple[str, object], ...] = (),
    depth: int = 2,
) -> Record:
    """A record for a call `depth` frames up."""
    frame = sys._getframe(depth)
    text, found = __render(msg, args)
    tags = fields.pop("tags", ())
    own = tuple((k, __value(v)) for k, v in fields.items())
    module = frame.f_globals.get("__name__", "?")
    return Record(
        level=level,
        target=target or module,
        msg=text,
        fields=bound + found + own,
        tags=tuple(tags) if not isinstance(tags, str) else (tags,),
        module=module,
        func=frame.f_code.co_name,
        file=frame.f_code.co_filename,
        line=frame.f_lineno,
    )


class Logger:
    """Makes records for one target, with fields of its own; `serlog.get` hands these out."""

    __slots__ = ("__fields", "__target")

    def __init__(self, target: str | None = None, **fields: object) -> None:
        self.__target = target
        self.__fields = tuple(fields.items())

    @property
    def target(self) -> str | None:
        """The target, or `None` for the calling module."""
        return self.__target

    def child(self, target: str | None = None, **fields: object) -> Logger:
        """A logger with more fields, and optionally another target."""
        out = Logger(target or self.__target)
        out.__fields = self.__fields + tuple(fields.items())
        return out

    bind = child

    def log(self, level: Levelish, msg: str | Template, /, *args: object, **fields: object) -> None:
        """Logs at `level`; `{}` in `msg` takes `args`, keywords become fields, `tags=` lists tags."""
        lvl = Level(level)
        if hub.enabled(lvl):
            hub.emit(make(lvl, self.__target, msg, args, fields, bound=self.__fields, depth=3))

    def __at(self, level: Level, msg: str | Template, args: tuple[object, ...], fields: dict[str, object]) -> None:
        if hub.enabled(level):
            hub.emit(make(level, self.__target, msg, args, fields, bound=self.__fields, depth=3))

    def trace(self, msg: str | Template, /, *args: object, **fields: object) -> None:
        """Logs at trace."""
        self.__at(Level.TRACE, msg, args, fields)

    def debug(self, msg: str | Template, /, *args: object, **fields: object) -> None:
        """Logs at debug."""
        self.__at(Level.DEBUG, msg, args, fields)

    def info(self, msg: str | Template, /, *args: object, **fields: object) -> None:
        """Logs at info."""
        self.__at(Level.INFO, msg, args, fields)

    def warn(self, msg: str | Template, /, *args: object, **fields: object) -> None:
        """Logs at warn."""
        self.__at(Level.WARN, msg, args, fields)

    def error(self, msg: str | Template, /, *args: object, **fields: object) -> None:
        """Logs at error."""
        self.__at(Level.ERROR, msg, args, fields)

    def fatal(self, msg: str | Template, /, *args: object, **fields: object) -> None:
        """Logs at fatal."""
        self.__at(Level.FATAL, msg, args, fields)

    def exception(self, msg: str | Template = "", /, *args: object, **fields: object) -> None:
        """Logs at error with the exception being handled as the `error` field."""
        e = sys.exception()
        self.__at(Level.ERROR, msg or (str(e) if e else "exception"), args, {"error": e, **fields})

    warning = warn
    critical = fatal


__root = Logger()


def get(target: str | None = None, **fields: object) -> Logger:
    """A logger for `target`, or for the calling module, carrying `fields` on every record."""
    return Logger(target, **fields)


def __level_call(level: Level) -> Callable[..., None]:
    def call(msg: str | Template, /, *args: object, **fields: object) -> None:
        if hub.enabled(level):
            hub.emit(make(level, None, msg, args, fields))

    call.__name__ = call.__qualname__ = level.name
    call.__doc__ = f"Logs at {level.name} for the calling module."
    return call


trace = __level_call(Level.TRACE)
debug = __level_call(Level.DEBUG)
info = __level_call(Level.INFO)
warn = warning = __level_call(Level.WARN)
error = __level_call(Level.ERROR)
fatal = critical = __level_call(Level.FATAL)


def log(level: Levelish, msg: str | Template, /, *args: object, **fields: object) -> None:
    """Logs at `level` for the calling module."""
    lvl = Level(level)
    if hub.enabled(lvl):
        hub.emit(make(lvl, None, msg, args, fields))


def exception(msg: str | Template = "", /, *args: object, **fields: object) -> None:
    """Logs at error with the exception being handled as the `error` field."""
    e = sys.exception()
    if hub.enabled(Level.ERROR):
        hub.emit(make(Level.ERROR, None, msg or (str(e) if e else "exception"), args, {"error": e, **fields}))


class catch(ContextDecorator):  # noqa: N801
    """Logs an exception escaping a function or block, then raises it again unless `reraise=False`.

    Works as `@catch`, `@catch(level="warn")`, `with catch():`, and on coroutine functions.
    """

    def __init__(self, fn: Callable | None = None, /, *, level: Levelish = Level.ERROR, reraise: bool = True, target: str | None = None) -> None:
        self.__level, self.__reraise, self.__target = Level(level), reraise, target
        self.__fn = fn
        if fn is not None:
            functools.update_wrapper(self, fn)

    def __call__(self, *a: object, **kw: object) -> object:
        if self.__fn is None:
            fn = a[0]
            return catch(fn, level=self.__level, reraise=self.__reraise, target=self.__target)  # type: ignore[arg-type]
        if inspect.iscoroutinefunction(self.__fn):

            async def wrapped() -> object:
                try:
                    return await self.__fn(*a, **kw)  # type: ignore[misc]
                except Exception as e:
                    return self.__report(e)

            return wrapped()
        try:
            return self.__fn(*a, **kw)
        except Exception as e:
            return self.__report(e)

    def __report(self, e: BaseException) -> None:
        name = getattr(self.__fn, "__qualname__", "block")
        if hub.enabled(self.__level):
            rec = Record(level=self.__level, target=self.__target or name, msg=f"{name} raised {type(e).__name__}: {e}", fields=(("error", e),))
            hub.emit(rec)
        if self.__reraise:
            raise e

    def __enter__(self) -> Self:
        return self

    def __exit__(self, kind: type[BaseException] | None, e: BaseException | None, tb: TracebackType | None) -> bool:
        if e is None or not isinstance(e, Exception):
            return False
        name = self.__target or "block"
        if hub.enabled(self.__level):
            hub.emit(Record(level=self.__level, target=name, msg=f"{name} raised {type(e).__name__}: {e}", fields=(("error", e),)))
        return not self.__reraise

    def __get__(self, obj: object, owner: type | None = None) -> object:
        return self if obj is None else functools.partial(self, obj)


def catch_uncaught() -> None:
    """Logs every uncaught exception, in any thread, at fatal before the previous hooks run."""
    import threading

    before, before_thread = sys.excepthook, threading.excepthook

    def hook(kind: type[BaseException], e: BaseException, tb: TracebackType | None) -> None:
        hub.emit(Record(level=Level.FATAL, target="uncaught", msg=f"{kind.__name__}: {e}", fields=(("error", e),)))
        hub.flush_slots(1)
        before(kind, e, tb)

    def thread_hook(args: threading.ExceptHookArgs) -> None:
        if args.exc_value is not None:
            name = args.thread.name if args.thread else "?"
            hub.emit(
                Record(
                    level=Level.FATAL,
                    target="uncaught",
                    msg=f"{args.exc_type.__name__}: {args.exc_value}",
                    fields=(("error", args.exc_value), ("thread", name)),
                )
            )
            hub.flush_slots(1)
        before_thread(args)

    sys.excepthook = hook
    threading.excepthook = thread_hook
