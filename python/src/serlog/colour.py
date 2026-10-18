"""Terminal colours, per level and overridable, honouring `NO_COLOR` and `FORCE_COLOR`."""

import os
from collections.abc import Callable, Mapping
from enum import StrEnum


class Colour(StrEnum):
    """A terminal colour."""

    PLAIN = "plain"
    RED = "red"
    GREEN = "green"
    YELLOW = "yellow"
    BLUE = "blue"
    MAGENTA = "magenta"
    CYAN = "cyan"
    WHITE = "white"
    GREY = "grey"
    ALARM = "alarm"

    @classmethod
    def _missing_(cls, value: object) -> Colour | None:
        return cls.GREY if value == "gray" else None

    @property
    def ansi(self) -> str:
        """The escape that starts it."""
        return {
            "plain": "",
            "red": "\x1b[1;31m",
            "green": "\x1b[32m",
            "yellow": "\x1b[33m",
            "blue": "\x1b[34m",
            "magenta": "\x1b[35m",
            "cyan": "\x1b[36m",
            "white": "\x1b[37m",
            "grey": "\x1b[90m",
            "alarm": "\x1b[1;37;41m",
        }[self.value]


class When(StrEnum):
    """When to colour."""

    AUTO = "auto"
    ALWAYS = "always"
    NEVER = "never"

    def resolve(self, tty: bool) -> bool:
        """Decides for a sink that is, or is not, a terminal."""
        match self:
            case When.ALWAYS:
                return True
            case When.NEVER:
                return False
        if os.environ.get("FORCE_COLOR"):
            return True
        return tty and not os.environ.get("NO_COLOR")


class Theme:
    """Colours by level; levels not listed keep their own."""

    __slots__ = ("__over",)

    def __init__(self, over: Mapping[object, Colour | str] | None = None) -> None:
        from .level import Level

        self.__over = {Level(k): Colour(v) for k, v in (over or {}).items()}

    def of(self, level: int) -> Colour:
        """The colour for `level`."""
        from .level import Level

        lvl = Level(level)
        return self.__over.get(lvl, lvl.colour)

    @property
    def overrides(self) -> dict[int, Colour]:
        """The levels given another colour."""
        return dict(self.__over)


def paint(on: bool, colour: Colour, text: str) -> str:
    """`text` in `colour` when `on`."""
    return f"{colour.ansi}{text}\x1b[0m" if on and colour is not Colour.PLAIN else text


type Painter = Callable[[str], str]
