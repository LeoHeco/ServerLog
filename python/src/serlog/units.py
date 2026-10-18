"""Durations and sizes as config files write them: `250ms`, `5s`, `2m`, `1h`, `1d`; `500KB`, `64MiB`."""

import re

__NUMBER = re.compile(r"\s*(\d+(?:\.\d+)?)\s*([A-Za-z]*)\s*")
__TIME = {"ms": 0.001, "": 1, "s": 1, "m": 60, "h": 3600, "d": 86400}
__SIZE = {"": 1, "b": 1, "k": 1000, "kb": 1000, "kib": 1 << 10, "m": 10**6, "mb": 10**6, "mib": 1 << 20, "g": 10**9, "gb": 10**9, "gib": 1 << 30}


def seconds(v: float | str) -> float:
    """A duration in seconds: a number, or text with `ms`, `s`, `m`, `h` or `d`."""
    if isinstance(v, int | float) and not isinstance(v, bool):
        if v < 0:
            raise ValueError(f"a duration cannot be negative: {v}")
        return float(v)
    m = __NUMBER.fullmatch(str(v))
    if not m or m[2] not in __TIME:
        raise ValueError(f"bad duration {v!r}")
    return float(m[1]) * __TIME[m[2]]


def size(v: int | str) -> int:
    """A byte count: a number, or text such as `500KB` or `64MiB`."""
    if isinstance(v, int) and not isinstance(v, bool):
        return v
    m = __NUMBER.fullmatch(str(v))
    if not m or m[2].lower() not in __SIZE:
        raise ValueError(f"bad size {v!r}")
    return int(float(m[1]) * __SIZE[m[2].lower()])


def show(secs: float) -> str:
    """The shortest way to write a duration that reads back the same."""
    ms = round(secs * 1000)
    for n, unit in ((86_400_000, "d"), (3_600_000, "h"), (60_000, "m"), (1000, "s")):
        if ms >= n and ms % n == 0:
            return f"{ms // n}{unit}"
    return f"{ms}ms"


def show_size(n: int) -> str | int:
    """The shortest way to write a byte count that reads back the same."""
    for m, unit in ((1 << 30, "GiB"), (1 << 20, "MiB"), (1 << 10, "KiB")):
        if n >= m and n % m == 0:
            return f"{n // m}{unit}"
    return n
