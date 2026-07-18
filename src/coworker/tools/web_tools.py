from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable

from coworker.core.types import ToolResult
from coworker.tools.base import PAGE_CHAR_LIMIT, Tool, ToolDefinition, paginate_text

_WEB_TOOL_MAX_ATTEMPTS = 3
_WEB_TOOL_RETRY_DELAYS = (0.5, 1.0)


async def _execute_with_retries(
    action: Callable[[], Awaitable[ToolResult]],
    *,
    tool_name: str,
    max_attempts: int = _WEB_TOOL_MAX_ATTEMPTS,
    retry_delays: tuple[float, ...] = _WEB_TOOL_RETRY_DELAYS,
) -> ToolResult:
    last_error: Exception | None = None

    for attempt in range(max_attempts):
        try:
            return await action()
        except Exception as e:
            last_error = e
            if attempt == max_attempts - 1:
                break
            await asyncio.sleep(retry_delays[min(attempt, len(retry_delays) - 1)])

    assert last_error is not None
    return ToolResult(
        tool_call_id="",
        content=f"{tool_name} failed after {max_attempts} attempts: {last_error}",
        is_error=True,
    )


class SearchWebTool(Tool):
    @property
    def definition(self) -> ToolDefinition:
        return ToolDefinition(
            name="search_web",
            description="搜索网络，返回相关结果摘要（支持多后端：bing/brave/duckduckgo/google）",
            parameters={
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "搜索关键词"},
                    "max_results": {"type": "integer", "description": "最多返回结果数，默认 5", "default": 5},
                },
                "required": ["query"],
            },
        )

    async def execute(self, query: str, max_results: int = 5, **_) -> ToolResult:
        async def action() -> ToolResult:
            from ddgs import DDGS

            results = DDGS().text(query, max_results=max_results, backend="auto")
            lines = [f"[{i+1}] {r['title']}\n{r['href']}\n{r['body']}" for i, r in enumerate(results)]
            return ToolResult(tool_call_id="", content="\n\n".join(lines) or "No results found.")

        return await _execute_with_retries(action, tool_name="search_web")


class FetchURLTool(Tool):
    @property
    def definition(self) -> ToolDefinition:
        return ToolDefinition(
            name="fetch_url",
            description=(
                "获取指定 URL 的网页内容，支持多种输出格式。"
                f"默认每页最多返回 {PAGE_CHAR_LIMIT} 字符，超出部分用 offset 翻页。"
            ),
            parameters={
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "要获取的 URL"},
                    "fmt": {
                        "type": "string",
                        "enum": ["text_markdown", "text_plain", "text_rich", "text"],
                        "description": "输出格式：text_markdown（默认）、text_plain、text_rich、text（原始 HTML）",
                        "default": "text_markdown",
                    },
                    "offset": {
                        "type": "integer",
                        "description": "字符偏移量（从 0 开始），用于翻页读取长网页，默认 0",
                    },
                    "limit": {
                        "type": "integer",
                        "description": f"最多返回的字符数，默认每页 {PAGE_CHAR_LIMIT}",
                    },
                },
                "required": ["url"],
            },
        )

    async def execute(
        self, url: str, fmt: str = "text_markdown", offset: int = 0, limit: int | None = None, **_
    ) -> ToolResult:
        async def action() -> ToolResult:
            from ddgs import DDGS

            result = DDGS().extract(url, fmt=fmt)
            content = result.get("content", "") if result else ""
            if isinstance(content, bytes):
                content = content.decode(errors="replace")
            if content:
                return ToolResult(tool_call_id="", content=paginate_text(content, offset, limit))
            return ToolResult(tool_call_id="", content="No content extracted.", is_error=True)

        return await _execute_with_retries(action, tool_name="fetch_url")
