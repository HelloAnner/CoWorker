class CoworkerError(Exception):
    pass


class ProviderError(CoworkerError):
    pass


class ProviderNotFoundError(ProviderError):
    pass


class ModelNotSupportedError(ProviderError):
    pass


class MemoryError(CoworkerError):
    pass


class ToolExecutionError(CoworkerError):
    def __init__(self, tool_name: str, reason: str) -> None:
        self.tool_name = tool_name
        self.reason = reason
        super().__init__(f"Tool '{tool_name}' failed: {reason}")


class RestartRequestedException(BaseException):
    """重启请求信号：由 RestartSelfTool 抛出，在 tool result 写入 short_term 之前传播。

    继承 BaseException 而非 Exception，使其穿透所有 `except Exception` 拦截（包括
    ToolRegistry、_act 等处的通用错误处理），直达 AgentLoop.run() 的专用 except 分支。
    """


