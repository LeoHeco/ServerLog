"""The reference sinks."""

from .console import Console
from .file import File, Rotate
from .loki import Loki
from .mail import Mail
from .memory import Memory

__all__ = ["Console", "File", "Loki", "Mail", "Memory", "Rotate"]
