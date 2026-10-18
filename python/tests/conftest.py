"""Shared helpers: the spec's vectors, a fresh interpreter for tests that start serlog, and clean slots."""

import json
import os
import subprocess
import sys
import textwrap
from pathlib import Path

import pytest

import serlog

SPEC = Path(__file__).parents[2] / "spec"


def vectors(name: str) -> dict:
    """One of the shared vector files."""
    return json.loads((SPEC / "cases" / f"{name}.json").read_text(encoding="utf-8"))


@pytest.fixture
def run(tmp_path: Path):
    """Runs `code` in a new interpreter inside `tmp_path`, returning stdout and stderr; fails on a non-zero exit."""

    def go(code: str, ok: bool = True, **env: str) -> tuple[str, str, int]:
        p = subprocess.run(
            [sys.executable, "-c", textwrap.dedent(code)],
            cwd=tmp_path,
            env={**os.environ, "PYTHONIOENCODING": "utf-8", **env},
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=60,
            check=False,
        )
        if ok:
            assert p.returncode == 0, p.stderr
        return p.stdout, p.stderr, p.returncode

    return go


@pytest.fixture
def clean():
    """Removes the slots a test added."""
    before = {s.name for s in serlog.status()}
    yield
    for s in serlog.status():
        if s.name not in before:
            serlog.remove(s.name)
