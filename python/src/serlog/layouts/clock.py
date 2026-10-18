"""How a record's time is written."""

import re
import time
from datetime import UTC, datetime

__START = time.monotonic()
__FRACTION = re.compile(r"%%|%\.(\d)f|%:z")


def elapsed() -> int:
    """Milliseconds since serlog was imported."""
    return int((time.monotonic() - __START) * 1000)


def strftime(t: datetime, fmt: str) -> str:
    """strftime that also takes `%.3f`, `%.6f`, `%.9f` for fractions and `%:z` for `+08:00`."""

    def piece(m: re.Match[str]) -> str:
        match m[0]:
            case "%%":
                return "%%"
            case "%:z":
                z = t.strftime("%z")
                return f"{z[:3]}:{z[3:]}" if z else ""
        return "." + f"{t.microsecond:06d}000"[: int(m[1])]

    return t.strftime(__FRACTION.sub(piece, fmt))


def iso(t: datetime, digits: int = 3) -> str:
    """UTC RFC 3339 with `digits` decimals: `2023-11-14T22:13:20.000Z`."""
    u = t.astimezone(UTC)
    frac = f"{u.microsecond:06d}000"[:digits]
    return f"{u:%Y-%m-%dT%H:%M:%S}{'.' + frac if digits else ''}Z"


def write(spec: str, t: datetime) -> str:
    """`t` in the format a `{time}` token asks for."""
    local = t.astimezone()
    match spec:
        case "" | "local":
            return strftime(local, "%Y-%m-%d %H:%M:%S%.3f")
        case "utc":
            return iso(t, 0)
        case "rfc3339":
            return strftime(local, "%Y-%m-%dT%H:%M:%S%.6f%:z")
        case "unix":
            return str(int(t.timestamp()))
        case "unix_ms":
            return str(int(t.timestamp() * 1000))
        case "unix_ns":
            return str(int(t.timestamp()) * 10**9 + t.microsecond * 1000)
        case fmt:
            return strftime(local, fmt)
