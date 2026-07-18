from __future__ import annotations

import time
from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

import explore_lab.orchestrator as orch_mod
from explore_lab.orchestrator import _dict_diff, create_app
from explore_lab.store import Branch, Experiment, Store


class _FakeResponse:
    def __init__(self, status_code: int = 200, json_data: dict | None = None, text: str = ""):
        self.status_code = status_code
        self._json = json_data if json_data is not None else {}
        self.text = text

    def json(self) -> dict:
        return self._json

    def raise_for_status(self) -> None:
        if self.status_code >= 400:
            raise httpx.HTTPStatusError("error", request=None, response=None)


class _FakeAsyncClient:
    def __init__(self, *args, **kwargs) -> None:
        pass

    async def __aenter__(self) -> _FakeAsyncClient:
        return self

    async def __aexit__(self, *exc) -> bool:
        return False

    async def get(self, url, **kwargs):
        if url.endswith("/state"):
            return _FakeResponse(200, {"status": "paused", "cycle_count": 0, "transcript": []})
        return _FakeResponse(200, {})

    async def post(self, url, **kwargs):
        return _FakeResponse(200, {"ok": True})

    async def patch(self, url, **kwargs):
        return _FakeResponse(200, {})


class _FakeProcess:
    def __init__(self, pid: int = 999999) -> None:
        self.pid = pid

    def poll(self):
        return None


def _seed_store(tmp_path: Path, source_workdir: Path) -> tuple[str, str]:
    store_path = tmp_path / "experiments.json"
    store = Store(store_path)
    experiment_id = "exp_seed"
    branch_id = "br_source"
    store.experiments[experiment_id] = Experiment(
        id=experiment_id, source_base_url="http://example.invalid", imported_at=0.0,
    )
    store.branches[branch_id] = Branch(
        id=branch_id, experiment_id=experiment_id, parent_id=None,
        workdir=str(source_workdir), control_port=1, pid=None,
        status="paused", is_baseline=True,
    )
    store._save_locked()
    return experiment_id, branch_id


def _write_index_html(ui_dir: Path) -> None:
    (ui_dir / "index.html").write_text(
        "<!doctype html><title>Explore Lab</title>",
        encoding="utf-8",
    )


@pytest.fixture
def app_client(tmp_path, monkeypatch):
    monkeypatch.setattr(httpx, "AsyncClient", _FakeAsyncClient)
    monkeypatch.setattr(orch_mod, "spawn_branch_runner", lambda workdir, port: _FakeProcess())

    async def _fake_wait_until_ready(port, timeout: float = 60.0):
        return {"status": "paused"}

    monkeypatch.setattr(orch_mod, "wait_until_ready", _fake_wait_until_ready)

    source_workdir = tmp_path / "source_workdir"
    source_workdir.mkdir()
    (source_workdir / "marker.txt").write_text("hello from source", encoding="utf-8")

    experiment_id, branch_id = _seed_store(tmp_path, source_workdir)

    app = create_app(data_root=tmp_path)
    with TestClient(app) as client:
        yield client, experiment_id, branch_id, source_workdir


class TestDictDiff:
    def test_flat_diff(self):
        diff = _dict_diff({"a": 1, "b": 2}, {"a": 1, "b": 3})
        assert diff == {"b": {"a": 2, "b": 3}}

    def test_nested_diff(self):
        diff = _dict_diff({"llm": {"default_model": "x"}}, {"llm": {"default_model": "y"}})
        assert diff == {"llm.default_model": {"a": "x", "b": "y"}}

    def test_no_diff_when_equal(self):
        assert _dict_diff({"a": {"b": 1}}, {"a": {"b": 1}}) == {}


class TestForkIsolation:
    def test_fork_copies_workdir_and_stays_isolated(self, app_client):
        client, _experiment_id, branch_id, source_workdir = app_client

        resp = client.post(f"/branches/{branch_id}/fork", json={"label": "试一个方向"})
        assert resp.status_code == 200
        new_branch_id = resp.json()["branch_id"]

        branch_resp = client.get(f"/branches/{new_branch_id}")
        assert branch_resp.status_code == 200
        new_workdir = Path(branch_resp.json()["workdir"])

        assert (new_workdir / "marker.txt").read_text(encoding="utf-8") == "hello from source"

        (new_workdir / "only_in_fork.txt").write_text("fork-only", encoding="utf-8")
        (source_workdir / "only_in_source.txt").write_text("source-only", encoding="utf-8")

        assert not (source_workdir / "only_in_fork.txt").exists()
        assert not (new_workdir / "only_in_source.txt").exists()

    def test_fork_registers_branch_with_correct_parent(self, app_client):
        client, experiment_id, branch_id, _source_workdir = app_client
        resp = client.post(f"/branches/{branch_id}/fork", json={"label": "b", "note": "n"})
        new_branch_id = resp.json()["branch_id"]

        branch = client.get(f"/branches/{new_branch_id}").json()
        assert branch["parent_id"] == branch_id
        assert branch["experiment_id"] == experiment_id
        assert branch["is_baseline"] is False
        assert branch["label"] == "b"
        assert branch["note"] == "n"

    def test_delete_branch_with_children_is_rejected(self, app_client):
        client, _experiment_id, branch_id, _source_workdir = app_client
        client.post(f"/branches/{branch_id}/fork", json={})

        resp = client.delete(f"/branches/{branch_id}")
        assert resp.status_code == 409

    def test_only_one_baseline_per_experiment(self, app_client):
        client, _experiment_id, branch_id, _source_workdir = app_client
        fork_resp = client.post(f"/branches/{branch_id}/fork", json={})
        new_branch_id = fork_resp.json()["branch_id"]

        client.patch(f"/branches/{new_branch_id}", json={"is_baseline": True})

        assert client.get(f"/branches/{new_branch_id}").json()["is_baseline"] is True
        assert client.get(f"/branches/{branch_id}").json()["is_baseline"] is False


class TestStartupRecovery:
    def test_wake_recovers_dead_branch_runner_and_migrated_workdir(self, tmp_path, monkeypatch):
        store = Store(tmp_path / "experiments.json")
        store.experiments["exp1"] = Experiment(
            id="exp1", source_base_url="http://example.invalid", imported_at=1.0,
        )
        branch_dir = tmp_path / "branches" / "br1"
        branch_dir.mkdir(parents=True)
        stale_workdir = tmp_path / "old-testbench" / "branches" / "br1"
        store.branches["br1"] = Branch(
            id="br1",
            experiment_id="exp1",
            parent_id=None,
            workdir=str(stale_workdir),
            control_port=1,
            pid=123,
            status="crashed",
        )
        store._save_locked()

        spawn_calls = []
        monkeypatch.setattr(orch_mod, "is_pid_alive", lambda pid: False)

        def _fake_spawn(workdir: Path, port: int):
            spawn_calls.append((workdir, port))
            return _FakeProcess(pid=4242)

        async def _fake_wait_until_ready(port, timeout: float = 60.0):
            return {"status": "paused"}

        monkeypatch.setattr(orch_mod, "spawn_branch_runner", _fake_spawn)
        monkeypatch.setattr(orch_mod, "wait_until_ready", _fake_wait_until_ready)

        app = create_app(data_root=tmp_path)
        with TestClient(app) as client:
            before = client.get("/branches/br1").json()
            resp = client.post("/branches/br1/wake")

        branch = resp.json()
        assert before["status"] == "stopped"
        assert spawn_calls
        assert spawn_calls[0][0] == branch_dir
        assert branch["workdir"] == str(branch_dir)
        assert branch["pid"] == 4242
        assert branch["status"] == "paused"
        assert branch["control_port"] == spawn_calls[0][1]

    def test_startup_does_not_spawn_branch_recovery(self, tmp_path, monkeypatch):
        store = Store(tmp_path / "experiments.json")
        store.experiments["exp1"] = Experiment(
            id="exp1", source_base_url="http://example.invalid", imported_at=1.0,
        )
        branch_dir = tmp_path / "branches" / "br1"
        branch_dir.mkdir(parents=True)
        store.branches["br1"] = Branch(
            id="br1",
            experiment_id="exp1",
            parent_id=None,
            workdir=str(branch_dir),
            control_port=1,
            pid=123,
            status="crashed",
        )
        store._save_locked()

        spawn_calls = []
        monkeypatch.setattr(orch_mod, "is_pid_alive", lambda pid: False)
        monkeypatch.setattr(
            orch_mod,
            "spawn_branch_runner",
            lambda workdir, port: spawn_calls.append((workdir, port)) or _FakeProcess(),
        )

        async def _slow_wait_until_ready(port, timeout: float = 60.0):
            await orch_mod.asyncio.sleep(10)
            return {"status": "paused"}

        monkeypatch.setattr(orch_mod, "wait_until_ready", _slow_wait_until_ready)

        app = create_app(data_root=tmp_path)
        started_at = time.monotonic()
        with TestClient(app) as client:
            branch = client.get("/branches/br1").json()
            elapsed = time.monotonic() - started_at

        assert elapsed < 1.0
        assert branch["status"] == "stopped"
        assert spawn_calls == []

    def test_wake_timeout_keeps_live_process_starting(self, tmp_path, monkeypatch):
        store = Store(tmp_path / "experiments.json")
        store.experiments["exp1"] = Experiment(
            id="exp1", source_base_url="http://example.invalid", imported_at=1.0,
        )
        branch_dir = tmp_path / "branches" / "br1"
        branch_dir.mkdir(parents=True)
        store.branches["br1"] = Branch(
            id="br1",
            experiment_id="exp1",
            parent_id=None,
            workdir=str(branch_dir),
            control_port=1,
            pid=None,
            status="crashed",
        )
        store._save_locked()

        monkeypatch.setattr(orch_mod, "is_pid_alive", lambda pid: False)
        monkeypatch.setattr(orch_mod, "spawn_branch_runner", lambda workdir, port: _FakeProcess())

        async def _timeout_wait_until_ready(port, timeout: float = 60.0):
            raise TimeoutError("still starting")

        monkeypatch.setattr(orch_mod, "wait_until_ready", _timeout_wait_until_ready)

        app = create_app(data_root=tmp_path)
        with TestClient(app) as client:
            resp = client.post("/branches/br1/wake")
            branch = client.get("/branches/br1").json()

        assert resp.status_code == 503
        assert branch["pid"] == 999999
        assert branch["status"] == "starting"


class TestStaticUi:
    def test_serves_index_and_assets(self, tmp_path):
        ui_dir = tmp_path / "ui"
        assets_dir = ui_dir / "assets"
        assets_dir.mkdir(parents=True)
        _write_index_html(ui_dir)
        (assets_dir / "app.js").write_text("console.log('ok');", encoding="utf-8")

        app = create_app(data_root=tmp_path / "data", ui_dir=ui_dir)
        with TestClient(app) as client:
            index_resp = client.get("/")
            assert index_resp.status_code == 200
            assert "Explore Lab" in index_resp.text
            assert index_resp.headers["content-type"].startswith("text/html")

            asset_resp = client.get("/assets/app.js")
            assert asset_resp.status_code == 200
            assert "console.log('ok');" in asset_resp.text

    def test_does_not_fallback_unknown_paths_to_index(self, tmp_path):
        ui_dir = tmp_path / "ui"
        ui_dir.mkdir()
        _write_index_html(ui_dir)

        app = create_app(data_root=tmp_path / "data", ui_dir=ui_dir)
        with TestClient(app) as client:
            resp = client.get("/not-found")
            assert resp.status_code == 404
            assert "Explore Lab" not in resp.text

    def test_api_routes_are_not_shadowed_by_static_ui(self, tmp_path):
        ui_dir = tmp_path / "ui"
        ui_dir.mkdir()
        _write_index_html(ui_dir)

        app = create_app(data_root=tmp_path / "data", ui_dir=ui_dir)
        with TestClient(app) as client:
            resp = client.get("/branches")
            assert resp.status_code == 200
            assert resp.json() == {"branches": []}


class TestStorePersistence:
    def test_round_trip(self, tmp_path):
        store_path = tmp_path / "experiments.json"
        store = Store(store_path)
        store.experiments["exp1"] = Experiment(
            id="exp1", source_base_url="http://x", imported_at=1.0,
        )
        store.branches["br1"] = Branch(
            id="br1", experiment_id="exp1", parent_id=None,
            workdir="w", control_port=1, pid=None, status="paused",
        )
        store._save_locked()

        reloaded = Store(store_path)
        assert "exp1" in reloaded.experiments
        assert reloaded.branches["br1"].workdir == "w"

