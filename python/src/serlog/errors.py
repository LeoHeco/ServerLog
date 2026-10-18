"""What can go wrong: at start-up, while registering a sink, and inside a sink."""


class SerlogError(Exception):
    """Anything serlog raises."""


class ConfigError(SerlogError, ValueError):
    """A setting is missing or malformed."""


class KindError(ConfigError):
    """A config names a kind nobody registered."""


class NameTaken(SerlogError):
    """Two sinks share a name, or a name is reserved."""


class Claimed(SerlogError):
    """A resource, such as a file, is already claimed by another sink."""


class NotReady(SerlogError):
    """A sink failed its start-up check."""

    def __init__(self, sink: str, cause: BaseException) -> None:
        super().__init__(f"sink `{sink}` is not ready: {cause}")
        self.sink = sink
        self.__cause__ = cause


class Rejected(SerlogError):
    """The other end refused the records; sending them again will not help."""


class Twice(SerlogError):
    """`init` ran a second time."""

    def __init__(self) -> None:
        super().__init__("serlog is already initialised")


def transient(e: BaseException) -> bool:
    """Whether trying again could succeed."""
    return not isinstance(e, Rejected | ConfigError)
