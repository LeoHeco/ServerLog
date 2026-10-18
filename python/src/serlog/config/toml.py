"""Writing the config tree as TOML; reading uses the standard library's `tomllib`."""

import json
import re
from collections.abc import Mapping, Sequence

__BARE = re.compile(r"^[A-Za-z0-9_-]+$")


def __key(k: str) -> str:
    return k if __BARE.match(k) else json.dumps(k, ensure_ascii=False)


def __inline(v: object) -> str:
    match v:
        case bool():
            return "true" if v else "false"
        case int() | float():
            return repr(v)
        case str():
            return json.dumps(v, ensure_ascii=False)
        case Mapping():
            return "{ " + ", ".join(f"{__key(str(k))} = {__inline(x)}" for k, x in v.items()) + " }" if v else "{}"
        case Sequence():
            return "[" + ", ".join(__inline(x) for x in v) + "]"
    raise ValueError(f"TOML cannot hold {v!r}")


def __table(path: str, m: Mapping[str, object], out: list[str]) -> None:
    plain = [(k, v) for k, v in m.items() if not isinstance(v, Mapping) and not __tables(v)]
    nested = [(k, v) for k, v in m.items() if isinstance(v, Mapping)]
    arrays = [(k, v) for k, v in m.items() if __tables(v)]
    if path and (plain or (not nested and not arrays)):
        out.append(f"[{path}]")
    out.extend(f"{__key(k)} = {__inline(v)}" for k, v in plain)
    for k, v in nested:
        out.append("")
        __table(f"{path}.{__key(k)}" if path else __key(k), v, out)  # type: ignore[arg-type]
    for k, v in arrays:
        for item in v:  # type: ignore[union-attr]
            out.append("")
            out.append(f"[[{path}.{__key(k)}]]" if path else f"[[{__key(k)}]]")
            out.extend(f"{__key(ik)} = {__inline(iv)}" for ik, iv in item.items())


def __tables(v: object) -> bool:
    return isinstance(v, list) and bool(v) and all(isinstance(x, Mapping) for x in v)


def dumps(tree: Mapping[str, object]) -> str:
    """`tree` as TOML; maps inside arrays of tables are written inline."""
    out: list[str] = []
    __table("", tree, out)
    return "\n".join(out).strip() + "\n"
