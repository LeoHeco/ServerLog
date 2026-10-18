"""Who is logging: application, environment and instance, fixed at start-up, and the host."""

import os
import socket
import sys
from dataclasses import dataclass
from functools import cache
from pathlib import Path
from typing import Self


@cache
def host() -> str:
    """The host name, as far as the environment tells."""
    return os.environ.get("HOSTNAME") or os.environ.get("COMPUTERNAME") or socket.gethostname()


@dataclass(frozen=True, slots=True)
class Meta:
    """Identity attached to records that leave the process."""

    app: str = ""
    env: str = ""
    instance: str = ""

    @classmethod
    def detect(cls) -> Self:
        """The program name and the host name."""
        app = Path(sys.argv[0]).stem if sys.argv and sys.argv[0] not in ("", "-c") else ""
        return cls(app=app, instance=host())


__current = Meta.detect()


def meta() -> Meta:
    """The identity given at start-up, or the defaults."""
    return __current


def set_meta(m: Meta) -> None:
    """Replaces the identity; `init` calls this."""
    global __current
    __current = m
