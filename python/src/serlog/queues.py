"""The shape of a queue in front of a sink."""

from dataclasses import dataclass
from enum import StrEnum

from .units import seconds


class Full(StrEnum):
    """What a full queue does with a new record."""

    DROP = "drop"
    BLOCK = "block"


@dataclass(frozen=True, slots=True)
class Queue:
    """Holds `size` records, written `batch` at a time or `wait` seconds after the first arrives."""

    size: int
    batch: int = 1000
    wait: float = 1.0
    full: Full = Full.DROP

    def __post_init__(self) -> None:
        object.__setattr__(self, "wait", seconds(self.wait))
        object.__setattr__(self, "full", Full(self.full))
