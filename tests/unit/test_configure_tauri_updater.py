import importlib.util
from pathlib import Path

import pytest

SCRIPT_PATH = Path(__file__).resolve().parents[2] / "scripts" / "configure_tauri_updater.py"


def load_script_module():
    spec = importlib.util.spec_from_file_location("configure_tauri_updater", SCRIPT_PATH)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_normalize_endpoint_appends_desktop_update_suffix_to_base_url():
    module = load_script_module()

    assert module.normalize_endpoint("https://coworker.example.com/") == (
        "https://coworker.example.com"
        "/api/desktop-updates/{{target}}/{{arch}}/{{current_version}}"
    )


def test_normalize_endpoint_keeps_tauri_placeholder_endpoint():
    module = load_script_module()
    endpoint = (
        "https://coworker.example.com/api/desktop-updates"
        "/{{target}}/{{arch}}/{{current_version}}/"
    )

    assert module.normalize_endpoint(endpoint) == endpoint.rstrip("/")


def test_normalize_endpoint_repairs_partial_tauri_placeholder_endpoint():
    module = load_script_module()

    assert module.normalize_endpoint(
        "http://updates.example.test:8000/api/desktop-updates/{{target}}/{{arch}}"
    ) == (
        "http://updates.example.test:8000"
        "/api/desktop-updates/{{target}}/{{arch}}/{{current_version}}"
    )


@pytest.mark.parametrize(
    "endpoint",
    ["updates.example.test", "https://updates.example.test?channel=stable"],
)
def test_normalize_endpoint_rejects_invalid_base_urls(endpoint):
    module = load_script_module()

    with pytest.raises(ValueError):
        module.normalize_endpoint(endpoint)
