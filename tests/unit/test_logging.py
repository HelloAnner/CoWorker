from __future__ import annotations

import logging

from loguru import logger

from coworker.core.logging import LoguruInterceptHandler


def test_standard_library_log_is_forwarded_to_loguru():
    messages: list[str] = []
    sink_id = logger.add(lambda message: messages.append(message.record["message"]))
    try:
        record = logging.LogRecord(
            name="mem0.memory.main",
            level=logging.WARNING,
            pathname=__file__,
            lineno=1,
            msg="Entity boost computation failed",
            args=(),
            exc_info=None,
        )
        LoguruInterceptHandler().handle(record)
    finally:
        logger.remove(sink_id)

    assert "Entity boost computation failed" in messages
