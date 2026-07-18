from __future__ import annotations

import uuid
from dataclasses import replace
from typing import TYPE_CHECKING

from coworker.core.types import CommunicateRequest, ToolResult

if TYPE_CHECKING:
    from coworker.tools.communicate_tool import CommunicateTool

DESKTOP_PREFIX = "coworker-desktop:"


class DesktopCommunicateSender:
    def __init__(self, communicate: CommunicateTool) -> None:
        self._communicate = communicate

    async def send(self, request: CommunicateRequest) -> ToolResult:
        queue = self._communicate.outbound_queue(request.participant_id)
        if queue is None:
            return ToolResult(
                tool_call_id="",
                content=f"该通信目标未连接: {request.participant_id}",
                is_error=True,
            )

        extra = dict(request.extra)
        request_id = str(extra.get("request_id") or uuid.uuid4())
        extra["request_id"] = request_id
        await queue.put(replace(request, extra=extra))

        content = f"Desktop 请求已发送 request_id={request_id}"
        if request.conversation_id:
            content += f"；conversation_id={request.conversation_id}"
        return ToolResult(tool_call_id="", content=content)
