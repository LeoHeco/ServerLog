"""The one base class every output derives from, built-in or not, and the entry it is handed."""

from abc import ABC, abstractmethod
from collections.abc import Callable, Mapping, Sequence
from typing import TYPE_CHECKING, ClassVar, TypedDict, Unpack

from .colour import Theme
from .layouts import INHERIT, Layout
from .queues import Queue
from .record import Record

if TYPE_CHECKING:
    from .alarms import Alarm
    from .filter import Filter
    from .processors import Step


class SlotKeys(TypedDict, total=False):
    """The slot settings every sink takes alongside its own, the same keys as in config files."""

    name: str
    level: int | str
    filter: str | Filter
    process: Sequence[Step | Mapping[str, object] | str]
    layout: Layout | str | Callable[[Record], str]
    format: str
    pattern: str
    schema: str
    keys: Mapping[str, str]
    levels: str
    nest: str
    colour: str
    colours: Mapping[str, str]
    alarm_only: bool
    on_fail: str | Alarm | None
    queue: int
    batch: int
    wait: float | str
    full: str
    retries: int
    probe: float | str


class Entry:
    """What a sink is handed: the record, and the line its slot's layout makes of it."""

    __slots__ = ("__cache", "__layout", "__line", "__theme", "record")

    def __init__(self, record: Record, layout: Layout = INHERIT, theme: Theme | None = None, cache: dict | None = None) -> None:
        self.record = record
        self.__layout, self.__theme, self.__cache = layout, theme, cache
        self.__line: str | None = None

    @property
    def coloured(self) -> bool:
        """Whether the line is coloured."""
        return self.__theme is not None

    @property
    def line(self) -> str:
        """The line, laid out on first use and shared with other slots using the same layout."""
        if self.__line is None:
            key = (id(self.__layout), id(self.__theme))
            if self.__cache is not None and key in self.__cache:
                self.__line = self.__cache[key]
            else:
                self.__line = self.__layout.render(self.record, self.__theme)
                if self.__cache is not None:
                    self.__cache[key] = self.__line
        return self.__line


class Sink(ABC):
    """Somewhere entries go.

    Only `emit` is required; queues, retries, health, alarms and layout come from the slot the sink is
    registered in, so a sink only delivers and raises what went wrong. Slot settings may be given to the
    constructor alongside the sink's own, so a config entry can be passed straight in: `File(**entry)`.
    """

    kind: ClassVar[str] = "custom"
    """The kind name, also the start of the default name."""

    def __init__(self, **slot: Unpack[SlotKeys]) -> None:
        if unknown := set(slot) - SlotKeys.__optional_keys__:
            raise TypeError(f"{type(self).__name__}() got an unexpected keyword argument '{sorted(unknown)[0]}'")
        self.__slot = slot

    @property
    def slot(self) -> SlotKeys:
        """The slot settings given with the sink."""
        return dict(getattr(self, "_Sink__slot", {}))  # type: ignore[return-value]

    @abstractmethod
    def emit(self, entry: Entry) -> None:
        """Delivers one entry; raise `Rejected` when trying again cannot help."""

    def emit_batch(self, entries: Sequence[Entry]) -> None:
        """Delivers several entries, as a queue hands them over; one by one unless overridden."""
        for e in entries:
            self.emit(e)

    def enabled(self, level: int, target: str) -> bool:
        """Whether records at `level` from `target` are wanted, checked before anything else."""
        return True

    def flush(self) -> None:
        """Pushes out anything buffered."""

    def close(self) -> None:
        """Releases what it holds, once its slot is removed or replaced."""

    def ready(self) -> None:
        """Checks at registration that the sink can work; raise to refuse."""

    def claim(self) -> str | None:
        """A resource only this sink may use, such as a file path."""
        return None

    def tty(self) -> bool:
        """Whether it writes to a terminal, for colour."""
        return False

    def default_layout(self) -> Layout:
        """The layout used when the slot sets none."""
        return INHERIT

    def queued(self) -> Queue | None:
        """The queue it runs behind when the slot sets none; `None` writes directly."""
        return None

    def options(self) -> dict[str, object] | None:
        """The config keys that rebuild this sink, without slot keys; `None` when it cannot be exported."""
        return None


class FunctionSink(Sink):
    """A sink made from a function taking an entry."""

    kind = "fn"

    def __init__(self, fn: Callable[[Entry], object], **slot: Unpack[SlotKeys]) -> None:
        super().__init__(**slot)
        self.__fn = fn

    def emit(self, entry: Entry) -> None:
        self.__fn(entry)
