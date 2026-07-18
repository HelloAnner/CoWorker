"""Experiment / Branch / Scenario 的本地 JSON 持久化。

量不大，先用 JSON + 文件锁（单 orchestrator 进程内用 asyncio.Lock 就够，
没有真的多进程并发写这个文件）。原子写：先写 tmp 再 replace，避免进程崩溃
写到一半留下损坏文件。
"""

from __future__ import annotations

import asyncio
import json
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class Experiment:
    id: str
    source_base_url: str
    imported_at: float


@dataclass
class Scenario:
    id: str
    experiment_id: str
    name: str
    events: list[dict[str, Any]]


@dataclass
class Branch:
    id: str
    experiment_id: str
    parent_id: str | None
    workdir: str
    control_port: int
    pid: int | None
    status: str
    label: str = ""
    note: str = ""
    is_baseline: bool = False
    verdict: dict[str, Any] | None = None
    # fork 时记录的差异（system_prompt_override / 热切 config 字段），仅供列表页快速展示；
    # 精确 diff 走 GET /branches/{id}/diff（读各分支当前磁盘/控制口的实际状态，不依赖这个字段）。
    overrides: dict[str, Any] = field(default_factory=dict)
    created_at: float = 0.0


class Store:
    """内存中的唯一真源 + 落盘镜像。所有读写都经同一个 asyncio.Lock 串行化。"""

    def __init__(self, path: Path) -> None:
        self._path = path
        self._lock = asyncio.Lock()
        self.experiments: dict[str, Experiment] = {}
        self.branches: dict[str, Branch] = {}
        self.scenarios: dict[str, Scenario] = {}
        self._load()

    def _load(self) -> None:
        if not self._path.is_file():
            return
        raw = json.loads(self._path.read_text(encoding="utf-8"))
        self.experiments = {
            k: Experiment(**v) for k, v in raw.get("experiments", {}).items()
        }
        self.branches = {k: Branch(**v) for k, v in raw.get("branches", {}).items()}
        self.scenarios = {k: Scenario(**v) for k, v in raw.get("scenarios", {}).items()}

    def _save_locked(self) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            "experiments": {k: asdict(v) for k, v in self.experiments.items()},
            "branches": {k: asdict(v) for k, v in self.branches.items()},
            "scenarios": {k: asdict(v) for k, v in self.scenarios.items()},
        }
        tmp = self._path.with_suffix(self._path.suffix + ".tmp")
        tmp.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
        tmp.replace(self._path)

    async def mutate(self, fn) -> Any:
        """在锁内执行 fn(self) 并落盘；fn 可以是同步函数，返回值原样透传。"""
        async with self._lock:
            result = fn(self)
            self._save_locked()
            return result

