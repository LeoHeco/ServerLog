"""Marks code running inside a sink or a processor, so records it makes are dropped instead of looping back."""

from collections.abc import Iterator
from contextlib import contextmanager
from contextvars import ContextVar
from typing import Protocol


class Counts(Protocol):
    """Something that counts suppressed records."""

    suppressed: int


__inside: ContextVar[tuple[bool, Counts | None]] = ContextVar("serlog_inside", default=(False, None))


@contextmanager
def inside(owner: Counts | None = None) -> Iterator[None]:
    """Marks the block as serlog's own code; records made in it count against `owner`."""
    token = __inside.set((True, owner))
    try:
        yield
    finally:
        __inside.reset(token)


def adopt(owner: Counts | None = None) -> None:
    """Marks the whole thread, for serlog's own background tasks."""
    __inside.set((True, owner))


def suppress() -> bool:
    """Counts and reports a record made inside serlog's own code, which must then be dropped."""
    busy, owner = __inside.get()
    if busy and owner is not None:
        owner.suppressed += 1
    return busy
