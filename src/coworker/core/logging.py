from __future__ import annotations

import inspect
import logging

from loguru import logger


class LoguruInterceptHandler(logging.Handler):
    """Forward standard-library logs from dependencies into Loguru sinks."""

    def emit(self, record: logging.LogRecord) -> None:
        try:
            level: str | int = logger.level(record.levelname).name
        except ValueError:
            level = record.levelno

        frame = inspect.currentframe()
        depth = 0
        while frame and (depth == 0 or frame.f_code.co_filename == logging.__file__):
            frame = frame.f_back
            depth += 1

        logger.opt(depth=depth, exception=record.exc_info).log(level, record.getMessage())


def intercept_standard_logging(level: int = logging.INFO) -> None:
    """Route dependency logs and Python warnings through the configured Loguru sinks."""
    root = logging.getLogger()
    root.handlers = [LoguruInterceptHandler()]
    root.setLevel(level)
    logging.captureWarnings(True)
