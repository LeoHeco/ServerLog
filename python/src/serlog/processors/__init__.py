"""Steps a record goes through before it is written: return it, a changed copy, or `None` to drop it.

Records are immutable, so a step that changes one returns a copy and every other slot still sees the original.
The hub runs one chain on every record; each slot may run one more before its sink.
"""

from collections.abc import Callable, Iterable

from ..record import Record
from .builtin import Dedupe, Exclude, Fields, Limit, Match, Processor, Redact, Sample, Sanitise

type Step = Processor | Callable[[Record], Record | None]
"""A built-in processor, or any function taking a record."""


def run(chain: Iterable[Step], rec: Record) -> Record | None:
    """`rec` after every step, or `None` once one dropped it."""
    for step in chain:
        out = step(rec)
        if out is None:
            return None
        rec = out
    return rec


__all__ = ["Dedupe", "Exclude", "Fields", "Limit", "Match", "Processor", "Redact", "Sample", "Sanitise", "Step", "run"]
