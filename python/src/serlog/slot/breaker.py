"""A slot's health: up, down, or probing after a failure; free of I/O so the shared vectors can drive it."""

from datetime import UTC, datetime
from enum import StrEnum
from typing import ClassVar


class Health(StrEnum):
    """Whether a sink is working."""

    UP = "up"
    DOWN = "down"
    PROBING = "probing"


class Tell(StrEnum):
    """How a failure is told."""

    REPORT = "report"
    SILENT = "silent"
    FOLDED = "folded"


class Phase(StrEnum):
    """Which report of an incident this is."""

    OPENED = "opened"
    REMINDER = "reminder"
    RESOLVED = "resolved"


class Breaker:
    """The state machine behind one slot; times are seconds on a monotonic clock."""

    __slots__ = ("__base", "__gap", "__next", "__opened_at", "__told", "error", "health", "since", "skipped")
    __LONGEST: ClassVar[float] = 300.0

    def __init__(self, probe: float, now: float) -> None:
        self.health = Health.UP
        self.__base = self.__gap = probe
        self.__next = now
        self.__told: float | None = None
        self.since = datetime.now(UTC)
        self.__opened_at = 0
        self.skipped = 0
        self.error: str | None = None

    @property
    def skipped_now(self) -> int:
        """Records skipped since the current failure began."""
        return self.skipped - self.__opened_at

    def admit(self, now: float) -> bool:
        """Whether a write may go ahead; turns a due `down` into `probing`, and counts a skip otherwise."""
        match self.health:
            case Health.UP:
                return True
            case Health.DOWN if now >= self.__next:
                self.health = Health.PROBING
                return True
        self.skipped += 1
        return False

    def resting(self, now: float) -> bool:
        """Whether the sink is left out at `now`, without counting anything."""
        match self.health:
            case Health.UP:
                return False
            case Health.DOWN:
                return now < self.__next
        return True

    def wait(self, now: float) -> float:
        """Seconds until the next probe."""
        return max(self.__next - now, 0.0)

    def passed(self) -> Phase | None:
        """A write succeeded; `resolved` when the failure had been told."""
        if self.health is Health.UP:
            return None
        self.health = Health.UP
        self.__gap = self.__base
        self.since = datetime.now(UTC)
        self.error = None
        told, self.__told = self.__told, None
        return Phase.RESOLVED if told is not None else None

    def failed(self, now: float, error: str, remind: float, tell: Tell) -> Phase | None:
        """A write failed; `opened` or `reminder` when that is to be told."""
        self.error = error
        match self.health:
            case Health.UP:
                self.health = Health.DOWN
                self.__gap = self.__base
                self.__next = now + self.__gap
                self.since = datetime.now(UTC)
                self.__opened_at = self.skipped
                if tell is Tell.SILENT:
                    return None
                self.__told = now
                return Phase.OPENED if tell is Tell.REPORT else None
            case Health.PROBING:
                self.health = Health.DOWN
                self.__gap = min(self.__gap * 2, self.__LONGEST)
                self.__next = now + self.__gap
                if tell is Tell.REPORT and self.__told is not None and now - self.__told >= remind:
                    self.__told = now
                    return Phase.REMINDER
        return None
