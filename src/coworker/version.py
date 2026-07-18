from __future__ import annotations

from importlib.metadata import PackageNotFoundError, version
from pathlib import Path


def get_version() -> str:
    try:
        return version("coworker")
    except PackageNotFoundError:
        root = Path(__file__).resolve().parents[2]
        version_file = root / "VERSION"
        try:
            return version_file.read_text(encoding="utf-8").strip()
        except OSError:
            return "0.0.0"


__version__ = get_version()
