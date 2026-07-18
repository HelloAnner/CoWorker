from __future__ import annotations

from coworker.skills.loader import SkillLoader
from coworker.tools.base import Tool, ToolDefinition, ToolResult


class GetSkillTool(Tool):
    def __init__(self, skill_loader: SkillLoader, agent_state=None) -> None:
        self._skill_loader = skill_loader
        self._state = agent_state

    @property
    def definition(self) -> ToolDefinition:
        return ToolDefinition(
            name="get_skill",
            description="加载指定 skill 的完整操作指南",
            parameters={
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "skill 名称，如 'plan'",
                    },
                },
                "required": ["skill_name"],
            },
        )

    async def execute(self, skill_name: str, **_) -> ToolResult:
        # 记录加载过的技能名到 agent state（状态记账，非运行日志事件——
        # 运行日志的「加载技能」行由前端从 get_skill 的 tool_call 事件派生）
        if self._state is not None:
            self._state.skill_load_counts[skill_name] = self._state.skill_load_counts.get(skill_name, 0) + 1
        skill = self._skill_loader.get(skill_name)
        if skill is None:
            available = ", ".join(self._skill_loader.list_names())
            return ToolResult(
                tool_call_id="",
                content=f"Skill '{skill_name}' not found. Available: {available}",
                is_error=True,
            )
        return ToolResult(tool_call_id="", content=skill.body)
