"""Standard output or standard error, one line per entry."""

import sys
from collections.abc import Sequence
from typing import Unpack, override

from ..compat import Stream, compat
from ..level import Level
from ..sink import Entry, Sink, SlotKeys


class Console(Sink):
    """Prints one line per entry; `stream` is `stdout`, `stderr` or `auto`, the profile's choice unless given."""

    kind = "console"

    def __init__(self, stream: Stream | str | None = None, **slot: Unpack[SlotKeys]) -> None:
        super().__init__(**slot)
        self.__stream = Stream(stream) if stream else None

    def __pick(self) -> Stream:
        return self.__stream or compat().stream

    def __err(self, level: int) -> bool:
        s = self.__pick()
        return s is Stream.STDERR or (s is Stream.AUTO and level >= Level.ERROR)

    @override
    def emit(self, entry: Entry) -> None:
        out = sys.stderr if self.__err(entry.record.level) else sys.stdout
        out.write(entry.line + "\n")

    @override
    def emit_batch(self, entries: Sequence[Entry]) -> None:
        out = [e.line + "\n" for e in entries if not self.__err(e.record.level)]
        err = [e.line + "\n" for e in entries if self.__err(e.record.level)]
        if out:
            sys.stdout.write("".join(out))
        if err:
            sys.stderr.write("".join(err))

    @override
    def flush(self) -> None:
        sys.stdout.flush()
        sys.stderr.flush()

    @override
    def tty(self) -> bool:
        s = sys.stderr if self.__pick() is Stream.STDERR else sys.stdout
        return hasattr(s, "isatty") and s.isatty()

    @override
    def options(self) -> dict[str, object]:
        return {"stream": self.__stream.value} if self.__stream else {}
