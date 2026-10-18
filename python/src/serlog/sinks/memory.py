"""A ring buffer in memory, for tests and for looking back at recent records."""

import threading
from collections import deque
from collections.abc import Callable
from typing import Unpack, override

from ..record import Record
from ..sink import Entry, Sink, SlotKeys


class Memory(Sink):
    """Keeps the newest `size` records and their lines."""

    kind = "memory"

    def __init__(self, size: int = 1000, **slot: Unpack[SlotKeys]) -> None:
        super().__init__(**slot)
        self.__size = size
        self.__kept: deque[tuple[Record, str]] = deque(maxlen=max(size, 1))
        self.__lock = threading.Lock()

    @override
    def emit(self, entry: Entry) -> None:
        with self.__lock:
            self.__kept.append((entry.record, entry.line))

    @property
    def records(self) -> list[Record]:
        """The records kept, oldest first."""
        with self.__lock:
            return [r for r, _ in self.__kept]

    @property
    def lines(self) -> list[str]:
        """The lines kept, oldest first."""
        with self.__lock:
            return [line for _, line in self.__kept]

    def find(self, test: Callable[[Record], bool]) -> list[Record]:
        """The records for which `test` holds."""
        return [r for r in self.records if test(r)]

    def clear(self) -> None:
        """Forgets everything."""
        with self.__lock:
            self.__kept.clear()

    def __len__(self) -> int:
        return len(self.__kept)

    @override
    def options(self) -> dict[str, object]:
        return {"size": self.__size}
