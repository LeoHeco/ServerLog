"""Severity: six built-in levels from 10 to 60, plus any defined at run time."""

import threading
from typing import ClassVar

from .colour import Colour
from .compat import Compat, compat


class Level(int):
    """How severe a record is; compares by value, and also takes a name such as `"warn"`."""

    __slots__ = ()

    TRACE: ClassVar[Level]
    DEBUG: ClassVar[Level]
    INFO: ClassVar[Level]
    WARN: ClassVar[Level]
    ERROR: ClassVar[Level]
    FATAL: ClassVar[Level]

    __BUILT_IN: ClassVar[dict[int, tuple[str, Colour]]] = {
        10: ("trace", Colour.CYAN),
        20: ("debug", Colour.MAGENTA),
        30: ("info", Colour.GREEN),
        40: ("warn", Colour.YELLOW),
        50: ("error", Colour.RED),
        60: ("fatal", Colour.ALARM),
    }
    __ALIASES: ClassVar[dict[str, int]] = {"warning": 40, "critical": 60}
    __custom: ClassVar[dict[int, tuple[str, Colour]]] = {}
    __lock: ClassVar[threading.Lock] = threading.Lock()

    def __new__(cls, value: int | str) -> Level:
        """A level by number, or by name in any case; `warning`, `critical` and defined names too."""
        if isinstance(value, Level):
            return value
        if isinstance(value, str):
            s = value.strip().lower()
            if s.isdigit():
                value = int(s)
            elif s in cls.__ALIASES:
                value = cls.__ALIASES[s]
            else:
                found = next((v for v, (n, _) in (cls.__BUILT_IN | cls.__custom).items() if n == s), None)
                if found is None:
                    raise ValueError(f"unknown level {value!r}: expected trace, debug, info, warn, error, fatal, a defined level or a number")
                value = found
        if not 0 <= value <= 255:
            raise ValueError(f"a level is 0 to 255, not {value}")
        return super().__new__(cls, value)

    @classmethod
    def define(cls, name: str, value: int, colour: Colour | str = Colour.PLAIN) -> Level:
        """Defines a level, or renames the one with that number."""
        if not name or not name.replace("_", "").isalnum() or name != name.lower() or name in ("off", *cls.__ALIASES):
            raise ValueError(f"bad level name {name!r}")
        if any(n == name for n, _ in cls.__BUILT_IN.values()):
            raise ValueError(f"{name!r} is a built-in level")
        with cls.__lock:
            cls.__custom = {v: e for v, e in cls.__custom.items() if v != value and e[0] != name} | {value: (name, Colour(colour))}
        return cls(value)

    @classmethod
    def defined(cls) -> dict[str, tuple[int, Colour]]:
        """Every level defined at run time, by name."""
        return {n: (v, c) for v, (n, c) in cls.__custom.items()}

    def __entry(self) -> tuple[str, Colour] | None:
        return self.__BUILT_IN.get(int(self)) or self.__custom.get(int(self))

    @property
    def name(self) -> str:
        """The lower-case name used in config files, filters and labels."""
        e = self.__entry()
        return e[0] if e else "custom"

    @property
    def colour(self) -> Colour:
        """Its colour in a terminal."""
        e = self.__entry()
        return e[1] if e else Colour.PLAIN

    def label(self, profile: Compat | None = None) -> str:
        """How a profile prints it, such as `WARNING` for Python's."""
        p = profile or compat()
        match p, int(self):
            case Compat.PYTHON, 40:
                return "WARNING"
            case Compat.PYTHON, 60:
                return "CRITICAL"
            case Compat.SLOG, 10:
                return "DEBUG-4"
            case Compat.SLOG, 60:
                return "ERROR+4"
        return self.name.upper()

    @property
    def syslog(self) -> int:
        """The syslog severity, for GELF."""
        v = int(self)
        return 7 if v <= 20 else 6 if v <= 30 else 4 if v <= 40 else 3 if v <= 50 else 2

    def __str__(self) -> str:
        return self.label()

    def __repr__(self) -> str:
        return f"Level({self.name}={int(self)})"

    def __format__(self, spec: str) -> str:
        return format(str(self), spec)


Level.TRACE, Level.DEBUG, Level.INFO, Level.WARN, Level.ERROR, Level.FATAL = (Level(v) for v in (10, 20, 30, 40, 50, 60))

type Levelish = Level | int | str
"""A level, a number or a name."""
