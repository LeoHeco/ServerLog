"""Standard-library `logging` and `warnings`, routed into serlog."""

import logging
from datetime import UTC, datetime
from typing import override

from . import hub
from .level import Level
from .record import Record


class Handler(logging.Handler):
    """A `logging.Handler` that sends every record into serlog; `extra=` keys become fields."""

    __PLAIN = frozenset(vars(logging.makeLogRecord({}))) | {"message", "asctime", "taskName"}

    @staticmethod
    def __level(n: int) -> Level:
        for floor, lvl in ((50, Level.FATAL), (40, Level.ERROR), (30, Level.WARN), (20, Level.INFO), (10, Level.DEBUG)):
            if n >= floor:
                return lvl
        return Level.TRACE

    @override
    def emit(self, record: logging.LogRecord) -> None:
        lvl = self.__level(record.levelno)
        if not hub.enabled(lvl):
            return
        fields: tuple[tuple[str, object], ...] = tuple((k, v) for k, v in vars(record).items() if k not in self.__PLAIN)
        if record.exc_info and record.exc_info[1] is not None:
            fields += (("error", record.exc_info[1]),)
        hub.emit(
            Record(
                level=lvl,
                target=record.name,
                msg=record.getMessage(),
                fields=fields,
                module=record.module,
                func=record.funcName,
                file=record.pathname,
                line=record.lineno,
                thread=record.threadName,
                task=getattr(record, "taskName", None),
                time=datetime.fromtimestamp(record.created, UTC),
            )
        )


def capture() -> None:
    """Makes serlog the only handler of the root `logging` logger and routes `warnings` through it too."""
    logging.basicConfig(handlers=[Handler()], level=logging.NOTSET, force=True)
    logging.captureWarnings(True)
