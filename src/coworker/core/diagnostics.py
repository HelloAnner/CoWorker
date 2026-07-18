"""运行时 asyncio 任务自省工具。

用于排查「进程卡在某个 await、迟迟到不了退出/重启那一步」这类问题：
枚举事件循环里仍然存活的 task，给出每个 task 当前挂起在哪一行。
既可在运行时通过调试接口查看，也可在关闭超时时写进日志定位元凶。
"""
from __future__ import annotations

import asyncio
import traceback


def _frame_location(frame) -> str:
    code = frame.f_code
    return f"{code.co_filename}:{frame.f_lineno} in {code.co_name}"


def task_snapshot(loop: asyncio.AbstractEventLoop | None = None) -> list[dict]:
    """枚举当前事件循环中所有 task 的轻量快照。

    每项含：name、coro 限定名、done/cancelled 状态、是否为当前 task，
    以及该 task 当前挂起所在的栈顶位置（waiting_at）——即它在 await 什么。
    """
    try:
        tasks = asyncio.all_tasks(loop) if loop is not None else asyncio.all_tasks()
    except RuntimeError:
        # 没有正在运行的事件循环
        return []

    try:
        current = asyncio.current_task(loop) if loop is not None else asyncio.current_task()
    except RuntimeError:
        current = None

    out: list[dict] = []
    for t in tasks:
        coro = t.get_coro()
        qualname = getattr(coro, "__qualname__", None) or repr(coro)
        # 栈顶（最内层）帧 = 当前挂起点；已完成的 task 返回空列表
        stack = t.get_stack(limit=1)
        waiting_at = _frame_location(stack[-1]) if stack else ""
        out.append({
            "name": t.get_name(),
            "coro": qualname,
            "done": t.done(),
            "cancelled": t.cancelled(),
            "current": t is current,
            "waiting_at": waiting_at,
        })
    # 未完成的排前面，便于一眼看到「卡住的」
    out.sort(key=lambda d: (d["done"], d["name"]))
    return out


def format_task_stacks(tasks: list[asyncio.Task] | None = None, *, limit: int = 12) -> str:
    """把指定（默认全部）未完成 task 的完整挂起栈格式化为多行文本，供日志输出。"""
    if tasks is None:
        try:
            tasks = list(asyncio.all_tasks())
        except RuntimeError:
            return "(no running event loop)"

    lines: list[str] = []
    for t in tasks:
        if t.done():
            continue
        lines.append(f"── task {t.get_name()!r} coro={getattr(t.get_coro(), '__qualname__', '?')}")
        frames = t.get_stack(limit=limit)
        if not frames:
            lines.append("    (no Python stack — likely waiting in C / not yet started)")
            continue
        for line in traceback.format_list(traceback.extract_stack(frames[-1])[-limit:]):
            lines.append("    " + line.rstrip())
    return "\n".join(lines) if lines else "(no pending tasks)"
