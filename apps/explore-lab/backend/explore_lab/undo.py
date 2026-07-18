"""每次手动 `_cycle()` 前的状态快照 + 有界撤销栈。

只回滚"对话/短期记忆状态"，不回滚该 cycle 里工具调用产生的真实副作用（写过的
文件、发过的消息、已经花掉的 LLM 调用费用）——这些和真实世界一样不可逆。

只维护"过去"栈，不维护"未来"栈：`back_step` 之后如果继续 `step`（而不是先
`fork`），被退回的状态直接作废，不能再"前进"恢复。
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass, field

from coworker.core.types import AgentState

_MAX_UNDO_DEPTH = 50

# AgentState 里随 _cycle() 演进、需要跟着回滚的字段（不含 current_provider/current_model：
# 模型切换是显式的用户操作，不应被"退回到上一轮对话状态"顺带撤销）。
_AGENT_STATE_FIELDS = (
    "is_running",
    "is_sleeping",
    "tick",
    "cycle_count",
    "last_active",
    "restart_requested",
)


@dataclass
class Snapshot:
    short_term_data: dict
    agent_state: dict
    thinking_md: str | None
    active_bubble_ids: frozenset[str]


def capture_agent_state(state: AgentState) -> dict:
    snap = {name: getattr(state, name) for name in _AGENT_STATE_FIELDS}
    snap["tool_call_counts"] = dict(state.tool_call_counts)
    snap["skill_load_counts"] = dict(state.skill_load_counts)
    return snap


def restore_agent_state(state: AgentState, snap: dict) -> None:
    for name in _AGENT_STATE_FIELDS:
        setattr(state, name, snap[name])
    state.tool_call_counts = dict(snap["tool_call_counts"])
    state.skill_load_counts = dict(snap["skill_load_counts"])


@dataclass
class UndoStack:
    max_depth: int = _MAX_UNDO_DEPTH
    _stack: deque[Snapshot] = field(default_factory=deque)

    def __post_init__(self) -> None:
        self._stack = deque(maxlen=self.max_depth)

    def push(self, snapshot: Snapshot) -> None:
        self._stack.append(snapshot)

    def pop(self) -> Snapshot | None:
        if not self._stack:
            return None
        return self._stack.pop()

    def __len__(self) -> int:
        return len(self._stack)

    def clear(self) -> None:
        self._stack.clear()
