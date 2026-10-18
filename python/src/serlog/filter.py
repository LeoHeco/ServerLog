"""Per-target levels written the `RUST_LOG` way: `info,app=debug,app.db=trace,hyper=off,web:*=warn/regex`."""

import fnmatch
import re
from dataclasses import dataclass

from .level import Level


@dataclass(frozen=True, slots=True)
class Rule:
    """One `target=level` directive; `None` is off."""

    target: str
    level: int | None

    @property
    def glob(self) -> bool:
        """Whether the target holds a wildcard."""
        return "*" in self.target

    def matches(self, target: str) -> bool:
        """Whether `target` is this one or below it."""
        if self.glob:
            return fnmatch.fnmatchcase(target, self.target)
        if not target.startswith(self.target):
            return False
        rest = target[len(self.target) :]
        return not rest or rest.startswith(("::", ".", ":", "/"))


class Filter:
    """Which records pass, by target, level and optionally message; the most specific directive wins."""

    __slots__ = ("__base", "__pattern", "__rules", "__spec")

    def __init__(self, spec: str | int | Level = "info") -> None:
        """Comma-separated directives; anything after the first `/` is a regex the message must match.

        As with `env_logger`, unmatched targets are off unless a default is given, and an empty spec lets errors through.
        """
        if not isinstance(spec, str):
            lvl = Level(spec)
            spec = lvl.name if lvl.name != "custom" else str(int(lvl))
        directives, slash, regex = spec.partition("/")
        self.__spec = spec
        self.__base: int | None = None if directives.strip() else int(Level.ERROR)
        rules: list[Rule] = []
        for d in (d.strip() for d in directives.split(",")):
            if not d:
                continue
            target, eq, lvl = d.partition("=")
            if eq:
                rules.append(Rule(target.strip(), self.__level(lvl.strip())))
                continue
            try:
                self.__base = self.__level(d)
            except ValueError:
                rules.append(Rule(d, 0))
        rules.sort(key=lambda r: (r.glob, -len(r.target.replace("*", ""))))
        self.__rules = tuple(rules)
        self.__pattern = re.compile(regex) if slash else None

    @staticmethod
    def __level(s: str) -> int | None:
        return None if s.lower() == "off" else int(Level(s))

    def allows(self, level: int, target: str) -> bool:
        """Whether a record at `level` from `target` passes, before its message is looked at."""
        rule = next((r for r in self.__rules if r.matches(target)), None)
        lvl = self.__base if rule is None else rule.level
        return lvl is not None and level >= lvl

    def admits(self, msg: str) -> bool:
        """Whether the message passes the `/regex` part, if there is one."""
        return self.__pattern is None or self.__pattern.search(msg) is not None

    @property
    def has_pattern(self) -> bool:
        """Whether there is a `/regex` part."""
        return self.__pattern is not None

    @property
    def floor(self) -> int | None:
        """The lowest level any target lets through, or `None` when everything is off."""
        levels = [lvl for lvl in (self.__base, *(r.level for r in self.__rules)) if lvl is not None]
        return min(levels, default=None)

    def __str__(self) -> str:
        return self.__spec

    def __repr__(self) -> str:
        return f"Filter({self.__spec!r})"

    def __eq__(self, other: object) -> bool:
        return isinstance(other, Filter) and str(self) == str(other)

    def __hash__(self) -> int:
        return hash(self.__spec)


def as_filter(f: Filter | str | int | Level) -> Filter:
    """A filter from a spec or a level."""
    return f if isinstance(f, Filter) else Filter(f)
