"""Fields bound to the current context, across threads and asyncio tasks alike."""

from collections.abc import Iterator
from contextlib import contextmanager
from contextvars import ContextVar

__bound: ContextVar[tuple[tuple[str, object], ...]] = ContextVar("serlog_bound", default=())


@contextmanager
def bind(**fields: object) -> Iterator[None]:
    """Adds `fields` to every record made inside the block; also works as a decorator."""
    token = __bound.set(__bound.get() + tuple(fields.items()))
    try:
        yield
    finally:
        __bound.reset(token)


def bound() -> tuple[tuple[str, object], ...]:
    """The fields bound here, in the order they were bound."""
    return __bound.get()
