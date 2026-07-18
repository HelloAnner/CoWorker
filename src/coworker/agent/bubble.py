from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from datetime import datetime
from typing import TYPE_CHECKING, Literal

if TYPE_CHECKING:
    from coworker.brain.brain import Brain
    from coworker.core.types import Message
    from coworker.memory.memory_tree import MemoryBlockTree


BubbleStatus = Literal["running", "done", "error", "cancelled", "timeout"]


@dataclass
class Bubble:
    id: str
    goal: str
    provider: str = ""
    model: str = ""
    status: BubbleStatus = "running"
    forked_context: list[Message] = field(default_factory=list)
    forked_tree: MemoryBlockTree | None = field(default=None, repr=False)
    inner_messages: list[Message] = field(default_factory=list)
    result: str = ""
    error: str = ""
    max_cycles: int = 10
    cycles_used: int = 0
    # 该泡泡服务的对象 id（用于续接路由；空表示非特定对象）。
    participant_id: str = ""
    # 挂载的宫殿名列表（续接路由时按宫殿/participant/目标配对）。
    palaces: list[str] = field(default_factory=list)
    # memory_tags 并集(来自挂载的宫殿),收尾时用于把结论按标签写回长期记忆。
    palace_tags: list[str] = field(default_factory=list)
    # 宫殿注入摘要(供泡泡日志记录:挂了哪些宫殿、强加载哪些 skill、召回了哪些记忆)。
    # 在 bubble_spawn 注入时填充,泡泡启动写日志时消费;None 表示未挂宫殿。
    palace_injection: dict | None = field(default=None, repr=False)
    created_at: datetime = field(default_factory=datetime.now)
    finished_at: datetime | None = None
    partial_results: list[str] = field(default_factory=list)
    checkpoint_count: int = 0
    initial_max_cycles: int = 0
    inbox: asyncio.Queue = field(default_factory=asyncio.Queue, repr=False)
    task: asyncio.Task | None = field(default=None, repr=False)
    brain: Brain | None = field(default=None, repr=False)

    def __post_init__(self) -> None:
        if self.initial_max_cycles <= 0:
            self.initial_max_cycles = self.max_cycles

    def is_terminal(self) -> bool:
        return self.status in ("done", "error", "cancelled", "timeout")

    def elapsed_seconds(self) -> float:
        if self.finished_at:
            return (self.finished_at - self.created_at).total_seconds()
        return (datetime.now() - self.created_at).total_seconds()


class BubbleStore:
    _MAX_HISTORY = 20

    def __init__(self, max_concurrent: int = 5) -> None:
        self._active: dict[str, Bubble] = {}
        self._history: list[Bubble] = []
        self.max_concurrent = max_concurrent

    def create(
        self,
        goal: str,
        forked_context: list[Message],
        max_cycles: int,
        provider: str = "",
        model: str = "",
    ) -> Bubble | str:
        if len(self._active) >= self.max_concurrent:
            active_ids = ", ".join(self._active.keys())
            return (
                f"已达到最大并发泡泡数（{self.max_concurrent}）。"
                f"当前活跃：{active_ids}。请等待或取消后再创建。"
            )
        ts = datetime.now().strftime("%y%m%d%H%M%S")
        bubble_id = f"bbl_{ts}"
        n = 2
        while bubble_id in self._active or any(b.id == bubble_id for b in self._history):
            bubble_id = f"bbl_{ts}_{n}"
            n += 1
        bubble = Bubble(
            id=bubble_id,
            goal=goal,
            provider=provider,
            model=model,
            forked_context=list(forked_context),
            max_cycles=max_cycles,
        )
        self._active[bubble_id] = bubble
        return bubble

    def get(self, bubble_id: str) -> Bubble | None:
        return self._active.get(bubble_id) or next(
            (b for b in self._history if b.id == bubble_id), None
        )

    def list_active(self) -> list[Bubble]:
        return list(self._active.values())

    def mark_done(self, bubble: Bubble) -> None:
        bubble.finished_at = datetime.now()
        self._active.pop(bubble.id, None)
        self._history.append(bubble)
        if len(self._history) > self._MAX_HISTORY:
            self._history = self._history[-self._MAX_HISTORY :]

    def cancel_all(self) -> None:
        for bubble in list(self._active.values()):
            if bubble.task and not bubble.task.done():
                bubble.task.cancel()
