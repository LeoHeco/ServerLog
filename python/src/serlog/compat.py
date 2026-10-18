"""Profiles that make serlog behave like another logger: its pattern, stream, default level and variables."""

from enum import StrEnum

PATTERNS = {
    "serlog": "{time} {level:<5} {target}: {msg}{fields}",
    "tracing": "{time:rfc3339} {level:>5} {spans}{target}: {msg}{fields}",
    "env_logger": "[{time:utc} {level:<5} {target}] {msg}",
    "python": "{level}:{target}:{msg}",
    "logback": "{time:%H:%M:%S%.3f} [{thread}] {level:<5} {target:~36} - {msg}",
    "slog": "time={time:%Y-%m-%dT%H:%M:%S%.3f%:z} level={level} msg={msg:q}{fields:q}",
    "pino": "[{time:%H:%M:%S%.3f}] {level} ({pid}): {msg}{fields}",
}
"""Patterns that reproduce other loggers' default output; `log4j` is another name for `logback`."""


class Stream(StrEnum):
    """Where a console writes."""

    STDOUT = "stdout"
    STDERR = "stderr"
    AUTO = "auto"


class Compat(StrEnum):
    """Another logger whose defaults serlog can take on."""

    SERLOG = "serlog"
    TRACING = "tracing"
    ENV_LOGGER = "env_logger"
    PYTHON = "python"
    LOGBACK = "logback"
    SLOG = "slog"
    PINO = "pino"

    @classmethod
    def _missing_(cls, value: object) -> Compat | None:
        return cls.LOGBACK if value == "log4j" else None

    @property
    def pattern(self) -> str:
        """The line pattern."""
        return PATTERNS[self.value]

    @property
    def stream(self) -> Stream:
        """Where the console writes."""
        return Stream.STDERR if self in (Compat.ENV_LOGGER, Compat.PYTHON) else Stream.STDOUT

    @property
    def level(self) -> int:
        """The level when nothing else is set."""
        return {Compat.ENV_LOGGER: 50, Compat.PYTHON: 40, Compat.LOGBACK: 20}.get(self, 30)

    @property
    def env(self) -> tuple[str, ...]:
        """Environment variables read for the filter, first set wins."""
        return {Compat.SERLOG: ("SERLOG_FILTER",), Compat.TRACING: ("RUST_LOG",), Compat.ENV_LOGGER: ("RUST_LOG",)}.get(self, ())


__current = Compat.SERLOG


def compat() -> Compat:
    """The profile in force."""
    return __current


def set_compat(profile: Compat | str) -> None:
    """Puts a profile in force; `init` calls this."""
    global __current
    __current = Compat(profile)
