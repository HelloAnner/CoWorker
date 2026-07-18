from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
SEMVER_RE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:[-+][0-9A-Za-z.-]+)?$")


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def check(label: str, actual: str, errors: list[str]) -> None:
    if actual != VERSION:
        errors.append(f"{label}: expected {VERSION}, got {actual}")


def match_toml_version(path: Path) -> str:
    match = re.search(r'^version = "([^"]+)"', path.read_text(encoding="utf-8"), re.MULTILINE)
    if not match:
        raise SystemExit(f"missing version in {path}")
    return match.group(1)


def match_uv_lock_version(path: Path) -> str:
    match = re.search(
        r'^\[\[package\]\]\nname = "coworker"\nversion = "([^"]+)"',
        path.read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    if not match:
        raise SystemExit(f"missing coworker version in {path}")
    return match.group(1)


def match_cargo_lock_version(path: Path, package: str) -> str:
    match = re.search(
        rf'^\[\[package\]\]\nname = "{package}"\nversion = "([^"]+)"',
        path.read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    if not match:
        raise SystemExit(f"missing {package} version in {path}")
    return match.group(1)


def main() -> int:
    errors: list[str] = []
    if not SEMVER_RE.match(VERSION):
        errors.append(f"VERSION is not SemVer: {VERSION}")

    check("pyproject.toml", match_toml_version(ROOT / "pyproject.toml"), errors)
    check("uv.lock", match_uv_lock_version(ROOT / "uv.lock"), errors)
    check("Cargo.toml", match_toml_version(ROOT / "Cargo.toml"), errors)
    for package in ("coworker-desktop-app", "coworker-desktop-core"):
        check(
            f"Cargo.lock {package}", match_cargo_lock_version(ROOT / "Cargo.lock", package), errors
        )
    check(
        "tauri.conf.json",
        read_json(ROOT / "apps/coworker-desktop/desktop/src-tauri/tauri.conf.json")["version"],
        errors,
    )
    for package in ("apps/coworker-desktop/desktop", "web"):
        check(
            f"{package}/package.json", read_json(ROOT / package / "package.json")["version"], errors
        )
        lock = read_json(ROOT / package / "package-lock.json")
        check(f"{package}/package-lock.json", lock["version"], errors)
        check(f"{package}/package-lock.json packages root", lock["packages"][""]["version"], errors)

    ref_name = os.environ.get("GITHUB_REF_NAME", "")
    if ref_name.startswith("coworker-desktop-v"):
        tag_version = ref_name.removeprefix("coworker-desktop-v")
        check("GITHUB_REF_NAME", tag_version, errors)

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print(f"version ok: {VERSION}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
