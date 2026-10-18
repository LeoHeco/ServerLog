"""Start-up from YAML, JSON or TOML, export of the settings in force, and reloading when the file changes.

The keys are those `serlog.init` takes, so `serlog.init(**tree)` and `serlog.load(path)` say the same thing.
"""

import json
import os
import re
import sys
import threading
import time
from collections.abc import Mapping
from pathlib import Path
from typing import TYPE_CHECKING, Literal
lazy import tomllib

lazy from ruamel.yaml import YAML

from ..errors import ConfigError
from . import kinds
from .toml import dumps as toml_dumps

if TYPE_CHECKING:
    from ..alarms import Alarm
    from ..processors import Step
    from ..sink import Sink

type Format = Literal["yaml", "json", "toml"]

__VAR = re.compile(r"\$\{([^}]*)\}")


def format_of(path: str | os.PathLike[str]) -> Format:
    """The format a file extension names; YAML for anything but `.json` and `.toml`."""
    suffix = Path(path).suffix.lower()
    return "json" if suffix == ".json" else "toml" if suffix == ".toml" else "yaml"


def read(text: str, format: Format = "yaml") -> dict[str, object]:
    """Config text as a tree, without expanding variables."""
    match format:
        case "json":
            tree = json.loads(text)
        case "toml":
            tree = tomllib.loads(text)
        case _:
            tree = YAML(typ="safe").load(text)
    if tree is None:
        return {}
    if not isinstance(tree, dict):
        raise ConfigError("the config must be a map")
    return tree


def expand[T](tree: T) -> T:
    """`${NAME}` in every string replaced by that environment variable; an unset one is an error."""

    def var(m: re.Match[str]) -> str:
        if (v := os.environ.get(m[1])) is None:
            raise ConfigError(f"environment variable {m[1]} is not set")
        return v

    match tree:
        case str():
            return __VAR.sub(var, tree)  # type: ignore[return-value]
        case dict():
            return {k: expand(v) for k, v in tree.items()}  # type: ignore[return-value]
        case list():
            return [expand(v) for v in tree]  # type: ignore[return-value]
    return tree


def __call(make: object, what: str, opts: Mapping[str, object]) -> object:
    try:
        return make(**opts)  # type: ignore[operator]
    except TypeError as e:
        m = re.search(r"unexpected keyword argument '(\w+)'", str(e))
        raise ConfigError(f"{what}: unknown key `{m[1]}`" if m else f"{what}: {e}") from e
    except ValueError as e:
        raise ConfigError(f"{what}: {e}") from e


def make_sink(entry: Mapping[str, object]) -> Sink:
    """The sink a config entry describes, slot keys included."""
    opts = dict(entry)
    kind = opts.pop("kind", None)
    if not isinstance(kind, str):
        raise ConfigError("each sink needs a `kind`")
    return __call(kinds.sink_maker(kind), f"{kind} sink", opts)  # type: ignore[return-value]


def make_step(spec: str | Mapping[str, object]) -> Step:
    """The processor a `process` entry describes: a name, or a map from a name to its options."""
    if isinstance(spec, str):
        name, opts = spec, {}
    elif len(spec) == 1:
        name, value = next(iter(spec.items()))
        opts = {} if value is True or value is None else value
        if not isinstance(opts, Mapping):
            raise ConfigError(f"processor `{name}` takes a map, not {value!r}")
    else:
        raise ConfigError(f"a processor is a name or a one-key map, not {spec!r}")
    return __call(kinds.step_maker(name), f"processor `{name}`", opts)  # type: ignore[return-value]


def make_alarm(name: str, spec: Mapping[str, object]) -> Alarm:
    """The alarm an `alarms` entry describes."""
    if name in ("panic", "ignore"):
        raise ConfigError(f"`{name}` cannot name an alarm")
    opts = dict(spec)
    kind = str(opts.pop("kind", "tiers"))
    return __call(kinds.alarm_maker(kind), f"alarm `{name}`", opts)  # type: ignore[return-value]


def loads(text: str, format: Format = "yaml") -> None:
    """Initialises from config text."""
    from ..start import start

    start(expand(read(text, format)))


def load(path: str | os.PathLike[str]) -> None:
    """Initialises from a file, the format from its extension; reloads it if it sets `watch`."""
    from ..start import start

    tree = expand(read(Path(path).read_text(encoding="utf-8"), format_of(path)))
    start(tree)
    if (every := tree.get("watch")) is not None:
        watch(Path(path), every)  # type: ignore[arg-type]


def watch(path: Path, every: float | str) -> None:
    """Reloads `path` whenever its modification time changes, if all of it passes."""
    from .. import hub
    from ..level import Level
    from ..record import Record
    from ..start import apply, plan
    from ..units import seconds

    gap = seconds(every)

    def stamp() -> float | None:
        try:
            return path.stat().st_mtime
        except OSError:
            return None

    seen = stamp()

    def run() -> None:
        nonlocal seen
        while True:
            time.sleep(gap)
            now = stamp()
            if now == seen:
                continue
            seen = now
            try:
                tree = expand(read(path.read_text(encoding="utf-8"), format_of(path)))
                apply(plan(tree, ()), replace=True)
            except Exception as e:
                rec = Record(level=Level.ERROR, target="serlog", msg=f"reloading {path} failed: {e}")
                try:
                    hub.BOARD.deliver("stderr", rec)
                except Exception:
                    print(rec.msg, file=sys.stderr)

    threading.Thread(target=run, name="serlog-watch", daemon=True).start()


def dump(format: Format = "yaml") -> str:
    """The settings in force as config text that loads back to the same behaviour; passwords come out as `***`."""
    from .export import tree

    t = tree()
    match format:
        case "json":
            return json.dumps(t, indent=2, ensure_ascii=False) + "\n"
        case "toml":
            return toml_dumps(t)
    import io

    out = io.StringIO()
    y = YAML(typ="safe", pure=True)
    y.default_flow_style = False
    y.sort_base_mapping_type_on_output = False
    y.dump(t, out)
    return out.getvalue()


__all__ = ["Format", "dump", "expand", "format_of", "kinds", "load", "loads", "make_alarm", "make_sink", "make_step", "read", "watch"]
