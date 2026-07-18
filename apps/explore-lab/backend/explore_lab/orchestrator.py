"""主服务：Experiment/Branch/Scenario 管理、导入/fork/replay/批量控制/对比/diff。

前端对单分支的 step/pause/resume/system_prompt_override/config 等控制调用直接打
分支自己的 control_port（`GET /branches/{id}` 里能拿到），不经过 orchestrator 转发；
orchestrator 只承担"需要跨分支信息"的事情：导入/fork/replay（涉及进程生命周期）、
批量并发控制、对比、diff、人工元信息（label/note/verdict）。
"""

from __future__ import annotations

import asyncio
import io
import json
import shutil
import time
import uuid
import zipfile
from contextlib import asynccontextmanager
from dataclasses import asdict
from pathlib import Path
from typing import Any

import httpx
import psutil
from fastapi import FastAPI, HTTPException, Query
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field

from explore_lab.process_manager import (
    control_base_url,
    fetch_state,
    find_free_port,
    is_pid_alive,
    kill_pid,
    spawn_branch_runner,
    wait_until_ready,
)
from explore_lab.store import Branch, Experiment, Scenario, Store

_DATA_ROOT = Path(__file__).resolve().parent.parent / "data"
_STORE_PATH = _DATA_ROOT / "experiments.json"
_BRANCHES_ROOT = _DATA_ROOT / "branches"
_DEFAULT_UI_DIR = Path(__file__).resolve().parents[2] / "frontend" / "dist"


def _new_id(prefix: str) -> str:
    return f"{prefix}_{uuid.uuid4().hex[:12]}"


def _branch_workdir(branch_id: str) -> Path:
    return _BRANCHES_ROOT / branch_id


def _dict_diff(a: dict, b: dict, prefix: str = "") -> dict[str, dict[str, Any]]:
    diff: dict[str, dict[str, Any]] = {}
    keys = set(a.keys()) | set(b.keys())
    for key in sorted(keys):
        path = f"{prefix}.{key}" if prefix else key
        av, bv = a.get(key), b.get(key)
        if isinstance(av, dict) and isinstance(bv, dict):
            diff.update(_dict_diff(av, bv, path))
        elif av != bv:
            diff[path] = {"a": av, "b": bv}
    return diff


def _find_stray_branch_runner_pids(known_pids: set[int]) -> list[int]:
    stray = []
    for proc in psutil.process_iter(["pid", "cmdline"]):
        try:
            cmdline = proc.info.get("cmdline") or []
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue
        is_branch_runner = any("explore_lab.branch_runner" in part for part in cmdline)
        if is_branch_runner and proc.info["pid"] not in known_pids:
            stray.append(proc.info["pid"])
    return stray


class ImportRequest(BaseModel):
    coworker_base_url: str
    admin_token: str


class ForkRequest(BaseModel):
    overrides: dict[str, Any] | None = None
    label: str = ""
    note: str = ""


class ReplayRequest(BaseModel):
    n: int = Field(gt=0)
    scenario_id: str


class ScenarioCreateRequest(BaseModel):
    name: str
    events: list[dict[str, Any]]


class BranchPatchRequest(BaseModel):
    label: str | None = None
    note: str | None = None
    verdict: dict[str, Any] | None = None
    is_baseline: bool | None = None


class BatchRequest(BaseModel):
    branch_ids: list[str]
    action: str
    params: dict[str, Any] | None = None


def create_app(data_root: Path | None = None, ui_dir: Path | None = None) -> FastAPI:
    data_root = data_root or _DATA_ROOT
    ui_dir = ui_dir or _DEFAULT_UI_DIR
    store_path = data_root / "experiments.json"
    branches_root = data_root / "branches"
    store = Store(store_path)
    wake_locks: dict[str, asyncio.Lock] = {}

    def branch_workdir(branch_id: str) -> Path:
        return branches_root / branch_id

    def _resolve_branch_workdir(branch: Branch) -> Path:
        stored = Path(branch.workdir)
        if stored.is_dir():
            return stored
        migrated = branch_workdir(branch.id)
        if migrated.is_dir():
            return migrated
        return stored

    async def _prepare_branch_recovery_state() -> None:
        def _mutate(s: Store) -> None:
            for b in s.branches.values():
                resolved = _resolve_branch_workdir(b)
                if Path(b.workdir) != resolved:
                    b.workdir = str(resolved)
                if is_pid_alive(b.pid):
                    continue
                b.pid = None
                b.status = "stopped" if resolved.is_dir() else "crashed"

        await store.mutate(_mutate)

    async def _cleanup_orphan_branch_runners() -> None:
        def _normalize_paths(s: Store) -> None:
            for b in s.branches.values():
                resolved = _resolve_branch_workdir(b)
                if Path(b.workdir) != resolved:
                    b.workdir = str(resolved)

        await store.mutate(_normalize_paths)
        known_pids = {b.pid for b in store.branches.values() if b.pid is not None}
        for pid in _find_stray_branch_runner_pids(known_pids):
            kill_pid(pid)

    async def _ensure_branch_running(branch_id: str) -> Branch:
        lock = wake_locks.setdefault(branch_id, asyncio.Lock())
        async with lock:
            branch = store.branches.get(branch_id)
            if branch is None:
                raise HTTPException(status_code=404, detail="branch not found")

            workdir = _resolve_branch_workdir(branch)
            if not workdir.is_dir():
                def _mark_missing(s: Store) -> None:
                    b = s.branches[branch_id]
                    b.workdir = str(workdir)
                    b.pid = None
                    b.status = "crashed"

                await store.mutate(_mark_missing)
                raise HTTPException(status_code=409, detail="branch workdir is missing")

            if Path(branch.workdir) != workdir:
                def _mark_resolved_path(s: Store) -> None:
                    s.branches[branch_id].workdir = str(workdir)

                await store.mutate(_mark_resolved_path)
                branch = store.branches[branch_id]

            if is_pid_alive(branch.pid):
                try:
                    state = await fetch_state(branch.control_port)
                except Exception:
                    kill_pid(branch.pid)
                else:
                    def _mark_live(s: Store) -> Branch:
                        b = s.branches[branch_id]
                        b.status = state.get("status", "unknown")
                        return b

                    return await store.mutate(_mark_live)

            port = find_free_port()
            proc = None
            try:
                proc = spawn_branch_runner(workdir, port)

                def _mark_starting(s: Store) -> None:
                    b = s.branches[branch_id]
                    b.workdir = str(workdir)
                    b.control_port = port
                    b.pid = proc.pid
                    b.status = "starting"

                await store.mutate(_mark_starting)
                state = await wait_until_ready(port)
            except asyncio.CancelledError:
                if proc is not None:
                    kill_pid(proc.pid)
                raise
            except Exception as e:
                proc_alive = proc is not None and proc.poll() is None
                recovered_pid = proc.pid if proc_alive and proc is not None else None

                def _mark_unready(s: Store) -> Branch:
                    b = s.branches[branch_id]
                    b.workdir = str(workdir)
                    if recovered_pid is not None:
                        b.control_port = port
                    b.pid = recovered_pid
                    b.status = "starting" if proc_alive else "crashed"
                    return b

                await store.mutate(_mark_unready)
                raise HTTPException(status_code=503, detail=f"branch did not become ready: {e}") from e

            def _mark_ready(s: Store) -> Branch:
                b = s.branches[branch_id]
                b.workdir = str(workdir)
                b.control_port = port
                b.pid = proc.pid
                b.status = state.get("status", "unknown")
                return b

            return await store.mutate(_mark_ready)

    async def _shutdown_branches() -> None:
        for b in list(store.branches.values()):
            if not is_pid_alive(b.pid):
                continue
            try:
                async with httpx.AsyncClient(timeout=5.0) as client:
                    await client.post(f"{control_base_url(b.control_port)}/control/pause")
            except Exception:
                pass
            kill_pid(b.pid)

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        await _prepare_branch_recovery_state()
        await _cleanup_orphan_branch_runners()
        try:
            yield
        finally:
            await _shutdown_branches()

    app = FastAPI(title="coworker-explore-lab orchestrator", lifespan=lifespan)
    app.add_middleware(
        CORSMiddleware, allow_origins=["*"], allow_methods=["*"], allow_headers=["*"],
    )

    index_html = ui_dir / "index.html"
    assets_dir = ui_dir / "assets"
    if assets_dir.is_dir():
        app.mount("/assets", StaticFiles(directory=assets_dir), name="explore_lab_assets")

    @app.get("/", include_in_schema=False)
    async def serve_ui_index():
        if not index_html.is_file():
            raise HTTPException(
                status_code=404,
                detail=f"Explore Lab UI is not built: {index_html}",
            )
        return FileResponse(index_html)

    async def _do_fork(
        source: Branch, overrides: dict[str, Any] | None, label: str, note: str,
    ) -> dict[str, Any]:
        try:
            async with httpx.AsyncClient(timeout=30.0) as client:
                await client.post(f"{control_base_url(source.control_port)}/snapshot")
        except httpx.HTTPError:
            pass  # 源分支可能已经不在了；尽力而为，fork 出来的是它磁盘上最后一次落盘的状态

        new_branch_id = _new_id("br")
        new_workdir = branch_workdir(new_branch_id)
        shutil.copytree(Path(source.workdir), new_workdir)

        port = find_free_port()
        proc = spawn_branch_runner(new_workdir, port)
        state = await wait_until_ready(port)

        overrides = overrides or {}
        async with httpx.AsyncClient(timeout=10.0) as client:
            if overrides.get("system_prompt_override") is not None:
                await client.patch(
                    f"{control_base_url(port)}/system_prompt_override",
                    json={"text": overrides["system_prompt_override"]},
                )
            if overrides.get("config"):
                await client.patch(f"{control_base_url(port)}/config", json=overrides["config"])

        now = time.time()

        def _mutate(s: Store) -> None:
            s.branches[new_branch_id] = Branch(
                id=new_branch_id,
                experiment_id=source.experiment_id,
                parent_id=source.id,
                workdir=str(new_workdir),
                control_port=port,
                pid=proc.pid,
                status=state.get("status", "unknown"),
                label=label,
                note=note,
                is_baseline=False,
                overrides=overrides,
                created_at=now,
            )

        await store.mutate(_mutate)
        return {"branch_id": new_branch_id, "control_port": port, "status": state.get("status")}

    @app.post("/experiments/import")
    async def import_experiment(payload: ImportRequest):
        base_url = payload.coworker_base_url.rstrip("/")
        try:
            async with httpx.AsyncClient(timeout=120.0) as client:
                resp = await client.get(
                    f"{base_url}/api/export_config",
                    headers={"Authorization": f"Bearer {payload.admin_token}"},
                )
        except httpx.HTTPError as e:
            raise HTTPException(
                status_code=502, detail=f"failed to reach coworker_base_url: {e}",
            ) from e

        if resp.status_code in (401, 403, 503):
            raise HTTPException(status_code=resp.status_code, detail=resp.text)
        if resp.status_code != 200:
            raise HTTPException(
                status_code=502, detail=f"unexpected export_config status: {resp.status_code}",
            )

        experiment_id = _new_id("exp")
        branch_id = _new_id("br")
        workdir = branch_workdir(branch_id)
        workdir.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(io.BytesIO(resp.content)) as zf:
            zf.extractall(workdir)

        port = find_free_port()
        proc = spawn_branch_runner(workdir, port)
        try:
            state = await wait_until_ready(port)
        except TimeoutError as e:
            raise HTTPException(status_code=500, detail=str(e)) from e

        now = time.time()

        def _mutate(s: Store) -> None:
            s.experiments[experiment_id] = Experiment(
                id=experiment_id, source_base_url=base_url, imported_at=now,
            )
            s.branches[branch_id] = Branch(
                id=branch_id,
                experiment_id=experiment_id,
                parent_id=None,
                workdir=str(workdir),
                control_port=port,
                pid=proc.pid,
                status=state.get("status", "unknown"),
                is_baseline=True,
                created_at=now,
            )

        await store.mutate(_mutate)
        return {
            "experiment_id": experiment_id,
            "branch_id": branch_id,
            "control_port": port,
            "status": state.get("status"),
        }

    @app.get("/experiments")
    async def list_experiments():
        out = []
        for exp in store.experiments.values():
            branch_count = sum(1 for b in store.branches.values() if b.experiment_id == exp.id)
            out.append({**asdict(exp), "branch_count": branch_count})
        return {"experiments": out}

    @app.get("/experiments/{experiment_id}")
    async def get_experiment(experiment_id: str):
        exp = store.experiments.get(experiment_id)
        if exp is None:
            raise HTTPException(status_code=404, detail="experiment not found")
        branches = [asdict(b) for b in store.branches.values() if b.experiment_id == experiment_id]
        return {"experiment": asdict(exp), "branches": branches}

    @app.post("/experiments/{experiment_id}/scenarios")
    async def create_scenario(experiment_id: str, payload: ScenarioCreateRequest):
        if experiment_id not in store.experiments:
            raise HTTPException(status_code=404, detail="experiment not found")
        scenario_id = _new_id("sc")

        def _mutate(s: Store) -> Scenario:
            scenario = Scenario(
                id=scenario_id, experiment_id=experiment_id,
                name=payload.name, events=payload.events,
            )
            s.scenarios[scenario_id] = scenario
            return scenario

        created = await store.mutate(_mutate)
        return asdict(created)

    @app.get("/experiments/{experiment_id}/scenarios")
    async def list_scenarios(experiment_id: str):
        return {
            "scenarios": [
                asdict(sc) for sc in store.scenarios.values() if sc.experiment_id == experiment_id
            ]
        }

    @app.post("/branches/{branch_id}/fork")
    async def fork_branch(branch_id: str, payload: ForkRequest = ForkRequest()):
        source = await _ensure_branch_running(branch_id)
        return await _do_fork(source, payload.overrides, payload.label, payload.note)

    @app.post("/branches/{branch_id}/wake")
    async def wake_branch(branch_id: str):
        branch = await _ensure_branch_running(branch_id)
        return asdict(branch)

    @app.post("/branches/{branch_id}/replay")
    async def replay_branch(branch_id: str, payload: ReplayRequest):
        source = await _ensure_branch_running(branch_id)
        scenario = store.scenarios.get(payload.scenario_id)
        if scenario is None:
            raise HTTPException(status_code=404, detail="scenario not found")

        new_branch_ids = []
        for i in range(payload.n):
            result = await _do_fork(source, None, f"replay#{i + 1} of {branch_id}", "")
            new_branch_ids.append(result["branch_id"])

        async def _drive(bid: str) -> None:
            branch = store.branches[bid]
            async with httpx.AsyncClient(timeout=30.0) as client:
                for event in scenario.events:
                    await client.post(
                        f"{control_base_url(branch.control_port)}/input",
                        json={
                            "content": event["content"],
                            "participant_id": event.get("participant_id", "explore_lab"),
                        },
                    )
                    delay = event.get("delay_after_seconds")
                    if delay:
                        await asyncio.sleep(delay)
                resume_url = f"{control_base_url(branch.control_port)}/control/resume"
                await client.post(resume_url, json={})

        await asyncio.gather(*(_drive(bid) for bid in new_branch_ids))
        return {"branch_ids": new_branch_ids}

    @app.get("/branches")
    async def list_branches():
        return {"branches": [asdict(b) for b in store.branches.values()]}

    @app.get("/branches/{branch_id}")
    async def get_branch(branch_id: str):
        branch = store.branches.get(branch_id)
        if branch is None:
            raise HTTPException(status_code=404, detail="branch not found")
        return asdict(branch)

    @app.patch("/branches/{branch_id}")
    async def patch_branch(branch_id: str, payload: BranchPatchRequest):
        if branch_id not in store.branches:
            raise HTTPException(status_code=404, detail="branch not found")

        def _mutate(s: Store) -> Branch:
            b = s.branches[branch_id]
            if payload.label is not None:
                b.label = payload.label
            if payload.note is not None:
                b.note = payload.note
            if payload.verdict is not None:
                b.verdict = payload.verdict
            if payload.is_baseline is True:
                for other in s.branches.values():
                    if other.experiment_id == b.experiment_id and other.id != b.id:
                        other.is_baseline = False
                b.is_baseline = True
            elif payload.is_baseline is False:
                b.is_baseline = False
            return b

        updated = await store.mutate(_mutate)
        return asdict(updated)

    @app.delete("/branches/{branch_id}")
    async def delete_branch(branch_id: str):
        branch = store.branches.get(branch_id)
        if branch is None:
            raise HTTPException(status_code=404, detail="branch not found")
        has_children = any(b.parent_id == branch_id for b in store.branches.values())
        if has_children:
            raise HTTPException(
                status_code=409, detail="cannot delete a branch that still has child branches",
            )
        kill_pid(branch.pid)
        shutil.rmtree(branch.workdir, ignore_errors=True)

        def _mutate(s: Store) -> None:
            del s.branches[branch_id]

        await store.mutate(_mutate)
        return {"deleted": True}

    @app.get("/branches/compare")
    async def compare_branches(ids: str = Query(...)):
        branch_ids = [x.strip() for x in ids.split(",") if x.strip()]
        branches = []
        for bid in branch_ids:
            b = store.branches.get(bid)
            if b is None:
                raise HTTPException(status_code=404, detail=f"branch not found: {bid}")
            branches.append(await _ensure_branch_running(bid))

        results: dict[str, Any] = {}

        async def _fetch(b: Branch) -> None:
            try:
                state = await fetch_state(b.control_port)
            except Exception as e:
                results[b.id] = {"status": "unreachable", "error": str(e)}
                return
            results[b.id] = {
                "label": b.label,
                "is_baseline": b.is_baseline,
                "verdict": b.verdict,
                "status": state.get("status"),
                "cycle_count": state.get("cycle_count"),
                "transcript": state.get("transcript", []),
            }

        await asyncio.gather(*(_fetch(b) for b in branches))
        return {"branches": results}

    @app.get("/branches/{branch_id}/diff")
    async def diff_branch(branch_id: str, against: str = Query(...)):
        a = await _ensure_branch_running(branch_id)
        b = await _ensure_branch_running(against)

        async def _snapshot(branch: Branch) -> dict[str, Any]:
            try:
                state = await fetch_state(branch.control_port)
            except Exception:
                state = {}
            thinking_path = Path(branch.workdir) / "data" / "thinking.md"
            thinking = thinking_path.read_text(encoding="utf-8") if thinking_path.is_file() else ""
            config_path = Path(branch.workdir) / "config.json"
            config = (
                json.loads(config_path.read_text(encoding="utf-8")) if config_path.is_file() else {}
            )
            return {
                "system_prompt_override_text": state.get("system_prompt_override_text"),
                "thinking_md": thinking,
                "config": config,
            }

        snap_a, snap_b = await asyncio.gather(_snapshot(a), _snapshot(b))
        return {
            "a": {"branch_id": a.id, **snap_a},
            "b": {"branch_id": b.id, **snap_b},
            "system_prompt_override_differs": (
                snap_a["system_prompt_override_text"] != snap_b["system_prompt_override_text"]
            ),
            "thinking_md_differs": snap_a["thinking_md"] != snap_b["thinking_md"],
            "config_diff": _dict_diff(snap_a["config"], snap_b["config"]),
        }

    @app.post("/experiments/{experiment_id}/batch")
    async def batch_action(experiment_id: str, payload: BatchRequest):
        results: dict[str, Any] = {}

        async def _one(bid: str) -> None:
            branch = store.branches.get(bid)
            if branch is None:
                results[bid] = {"ok": False, "error": "branch not found"}
                return
            try:
                branch = await _ensure_branch_running(bid)
            except HTTPException as e:
                results[bid] = {"ok": False, "error": e.detail}
                return
            url = f"{control_base_url(branch.control_port)}/control/{payload.action}"
            try:
                async with httpx.AsyncClient(timeout=120.0) as client:
                    resp = await client.post(url, json=payload.params or {})
                results[bid] = {
                    "ok": resp.status_code < 400,
                    "status_code": resp.status_code,
                    "body": resp.json(),
                }
            except Exception as e:
                results[bid] = {"ok": False, "error": str(e)}

        await asyncio.gather(*(_one(bid) for bid in payload.branch_ids))
        return {"results": results}

    return app


def main() -> None:
    import argparse

    import uvicorn

    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8100)
    parser.add_argument("--ui-dir", type=Path, default=_DEFAULT_UI_DIR)
    args = parser.parse_args()

    app = create_app(ui_dir=args.ui_dir)
    uvicorn.run(app, host="127.0.0.1", port=args.port, log_level="info")


if __name__ == "__main__":
    main()
