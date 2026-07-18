#!/usr/bin/env python3
"""Inject release-time Tauri updater settings into the desktop config."""

from __future__ import annotations

import json
import os
from pathlib import Path
from urllib.parse import urlsplit, urlunsplit

ROOT = Path(__file__).resolve().parents[1]
TAURI_CONFIG = ROOT / "apps" / "coworker-desktop" / "desktop" / "src-tauri" / "tauri.conf.json"
DEFAULT_ENDPOINT = (
    "https://coworker.example.com/api/desktop-updates/{{target}}/{{arch}}/{{current_version}}"
)
ENDPOINT_SUFFIX = "/api/desktop-updates/{{target}}/{{arch}}/{{current_version}}"
PLACEHOLDER_PUBLIC_KEY = "REPLACE_WITH_TAURI_UPDATER_PUBLIC_KEY"


def normalize_endpoint(endpoint: str) -> str:
    endpoint = endpoint.strip()
    try:
        parsed = urlsplit(endpoint)
        _ = parsed.port
    except ValueError as error:
        raise ValueError("updater endpoint must be an absolute HTTP(S) URL") from error
    if (
        parsed.scheme.lower() not in {"http", "https"}
        or not parsed.hostname
        or any(char.isspace() for char in parsed.netloc)
    ):
        raise ValueError("updater endpoint must be an absolute HTTP(S) URL")
    if parsed.fragment:
        raise ValueError("updater endpoint must not include a fragment")
    if all(
        placeholder in endpoint for placeholder in ("{{target}}", "{{arch}}", "{{current_version}}")
    ):
        return urlunsplit(parsed._replace(path=parsed.path.rstrip("/")))
    if parsed.query:
        raise ValueError("updater base URL must not include a query")
    base_path = parsed.path.partition("/api/desktop-updates")[0].rstrip("/")
    base_url = urlunsplit((parsed.scheme, parsed.netloc, base_path, "", ""))
    return f"{base_url}{ENDPOINT_SUFFIX}"


def main() -> int:
    pubkey = os.environ.get("TAURI_UPDATER_PUBLIC_KEY", "").strip()
    endpoint = os.environ.get("TAURI_UPDATER_ENDPOINT", "").strip()
    if not pubkey:
        raise SystemExit("TAURI_UPDATER_PUBLIC_KEY is required")
    if not endpoint:
        raise SystemExit("TAURI_UPDATER_ENDPOINT is required")
    try:
        endpoint = normalize_endpoint(endpoint)
    except ValueError as error:
        raise SystemExit(str(error)) from error

    config = json.loads(TAURI_CONFIG.read_text(encoding="utf-8"))
    updater = config.setdefault("plugins", {}).setdefault("updater", {})
    updated = False
    if pubkey:
        updater["pubkey"] = pubkey
        updated = True
        print("configured updater pubkey")
    if endpoint:
        updater["endpoints"] = [endpoint]
        updated = True
        print(f"configured updater endpoint: {endpoint}")
    if updated:
        TAURI_CONFIG.write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")
    if pubkey == PLACEHOLDER_PUBLIC_KEY or endpoint == DEFAULT_ENDPOINT:
        raise SystemExit("updater config still contains placeholder values")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
