"""spawn/kill branch_runner 子进程 + 端口分配 + 健康检查。

每个分支必须是独立 OS 进程（各自独立 cwd）——同进程内多份对象图会在
`os.getcwd()`/模块级路径上互相踩踏，这是本次架构的硬约束。
"""

from __future__ import annotations

import socket
import subprocess
import sys
import time
from pathlib import Path

import httpx
import psutil

_READY_POLL_INTERVAL = 0.2


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        s.listen(1)
        return s.getsockname()[1]


def spawn_branch_runner(workdir: Path, control_port: int) -> subprocess.Popen:
    workdir.mkdir(parents=True, exist_ok=True)
    log_dir = workdir / "data" / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    stdout_log = (log_dir / "branch_runner.stdout.log").open("ab")
    return subprocess.Popen(
        [
            sys.executable, "-m", "explore_lab.branch_runner",
            "--workdir", str(workdir),
            "--control-port", str(control_port),
        ],
        cwd=str(workdir),
        stdout=stdout_log,
        stderr=subprocess.STDOUT,
    )


def is_pid_alive(pid: int | None) -> bool:
    if pid is None:
        return False
    return psutil.pid_exists(pid)


def kill_pid(pid: int | None, timeout: float = 5.0) -> None:
    if pid is None:
        return
    try:
        proc = psutil.Process(pid)
    except psutil.NoSuchProcess:
        return
    try:
        proc.terminate()
        proc.wait(timeout=timeout)
    except psutil.TimeoutExpired:
        proc.kill()
    except psutil.NoSuchProcess:
        pass


def control_base_url(control_port: int) -> str:
    return f"http://127.0.0.1:{control_port}"


async def wait_until_ready(control_port: int, timeout: float = 60.0) -> dict:
    """轮询分支的 /state 直到不再是 starting（成功 paused 或失败 crashed），超时抛错。"""
    deadline = time.monotonic() + timeout
    last_state: dict = {"status": "starting"}
    async with httpx.AsyncClient(timeout=5.0) as client:
        while time.monotonic() < deadline:
            try:
                resp = await client.get(f"{control_base_url(control_port)}/state")
                if resp.status_code == 200:
                    last_state = resp.json()
                    if last_state.get("status") != "starting":
                        return last_state
            except httpx.HTTPError:
                pass
            import asyncio

            await asyncio.sleep(_READY_POLL_INTERVAL)
    raise TimeoutError(
        f"branch on port {control_port} did not become ready within {timeout}s: {last_state}"
    )


async def fetch_state(control_port: int) -> dict:
    async with httpx.AsyncClient(timeout=10.0) as client:
        resp = await client.get(f"{control_base_url(control_port)}/state")
        resp.raise_for_status()
        return resp.json()

