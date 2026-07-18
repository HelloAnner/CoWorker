from __future__ import annotations

import json
import os
import re
import time
from pathlib import Path

from coworker.channels.desktop import DesktopDispatcher, DesktopRegistry
from coworker.core.types import IncomingEvent
from coworker.memory.short_term import ShortTermMemory


def _detail_path_from(content: str) -> str:
    match = re.search(r"见文件：(.*?)，可用 read_file", content)
    assert match, f"no detail-file pointer in content: {content!r}"
    return match.group(1)


def _envelope(
    event_type: str,
    payload: dict | None = None,
    *,
    conversation_id: str | None = None,
    request_id: str | None = None,
) -> str:
    envelope: dict = {
        "protocol_version": 1,
        "message_id": "msg-1",
        "created_at": "2026-07-15T00:00:00Z",
        "type": event_type,
        "payload": payload or {},
    }
    if request_id is not None:
        envelope["request_id"] = request_id
    if conversation_id is not None:
        envelope["conversation_id"] = conversation_id
    return json.dumps(envelope, ensure_ascii=False)


def _event(
    content: str,
    *,
    participant_id: str = "cw-desktop:desk:claude:cw:p",
    conversation_id: str | None = None,
) -> IncomingEvent:
    return IncomingEvent(
        participant_id=participant_id,
        content=content,
        conversation_id=conversation_id,
    )


def _dispatcher(tmp_path) -> DesktopDispatcher:
    return DesktopDispatcher(DesktopRegistry(ShortTermMemory(), tmp_path))


def test_snapshot_is_consumed_and_feeds_registry(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    payload = {"desktop_id": "desk-a", "actor_id": "claude", "display_name": "Desk A"}
    event = _event(_envelope("desktop.actor.snapshot", payload))

    assert dispatcher(event) is True
    assert "desk-a:claude" in dispatcher._registry.actors


def test_command_result_ok_is_suppressed(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    event = _event(_envelope("desktop.command.result", {"request_id": "r-1", "ok": True}))
    assert dispatcher(event) is True


def test_command_result_failure_is_rendered_and_wakes_agent(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    event = _event(_envelope("desktop.command.result", {"request_id": "r-1", "ok": False}))

    assert dispatcher(event) is False
    assert "错误" in event.content
    assert "r-1" not in event.content or "错误" in event.content  # content rewritten


def test_server_request_resolved_is_suppressed(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    event = _event(
        _envelope("desktop.server_request.resolved", {"server_request_id": "0", "params": {}})
    )
    assert dispatcher(event) is True


def test_error_is_rendered_and_wakes_agent(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    event = _event(_envelope("desktop.error", {"message": "boom"}))

    assert dispatcher(event) is False
    assert "错误" in event.content
    assert "boom" in event.content


def test_codex_approval_renders_server_request_id_template(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    payload = {
        "codex_id": "codex-local",
        "server_request_id": "srv-123",
        "method": "commandExecution",
        "params": {"command": "git push"},
        "status": "pending",
    }
    event = _event(
        _envelope("desktop.approval.requested", payload, conversation_id="thread-1"),
        conversation_id="thread-1",
    )

    assert dispatcher(event) is False
    assert "审批请求" in event.content
    assert "Codex" in event.content
    assert "server_request_id" in event.content
    assert "srv-123" in event.content
    assert "decision" in event.content
    assert "communicate(" in event.content
    assert "thread-1" in event.content


def test_claude_approval_renders_request_id_template(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    payload = {
        "actor_id": "claude",
        "request_id": "req-abc",
        "session_id": "session-1",
        "tool_name": "Bash",
        "input": {"command": "rm -rf /"},
    }
    event = _event(
        _envelope("desktop.approval.requested", payload, conversation_id="session-1"),
        conversation_id="session-1",
    )

    assert dispatcher(event) is False
    assert "Claude" in event.content
    assert "request_id" in event.content
    assert "req-abc" in event.content
    assert "decision" in event.content


def test_claude_askuserquestion_renders_questions_and_answers_template(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    payload = {
        "actor_id": "claude",
        "request_id": "q-1",
        "session_id": "session-1",
        "tool_name": "AskUserQuestion",
        "input": {
            "questions": [
                {
                    "question": "Which database should we use?",
                    "options": [{"label": "SQLite", "description": "local file"}],
                }
            ]
        },
    }
    event = _event(
        _envelope("desktop.user_input.requested", payload, conversation_id="session-1"),
        conversation_id="session-1",
    )

    assert dispatcher(event) is False
    assert "提问请求" in event.content
    assert "Which database should we use?" in event.content
    assert "SQLite" in event.content
    assert "user_input_request_id" in event.content
    assert "q-1" in event.content
    assert "answers" in event.content


def test_thread_event_envelope_falls_back_to_message(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    event = _event(_envelope("desktop.thread.event", {"message": "hello there"}))

    assert dispatcher(event) is False
    assert event.content == "hello there"


def test_non_json_content_passes_through(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    event = _event("just a plain chat message")

    assert dispatcher(event) is False
    assert event.content == "just a plain chat message"


def test_unknown_desktop_type_passes_through(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    original = _envelope("desktop.something.new", {"foo": "bar"})
    event = _event(original)

    assert dispatcher(event) is False
    assert event.content == original


def test_unsupported_protocol_version_is_consumed(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    raw = json.dumps(
        {
            "protocol_version": 99,
            "message_id": "m",
            "type": "desktop.command.result",
            "payload": {"ok": True},
        }
    )
    event = _event(raw)

    assert dispatcher(event) is True


def test_short_error_is_not_folded(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    event = _event(_envelope("desktop.error", {"message": "boom"}, request_id="err-2"))

    assert dispatcher(event) is False
    assert event.content == "[CoWorker Desktop 错误]\n内容：boom"
    assert "read_file" not in event.content


def test_long_error_is_folded_with_read_file_pointer(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    long_message = "boom-" * 200  # ~1000 chars, well past the fold threshold
    event = _event(_envelope("desktop.error", {"message": long_message}, request_id="err-1"))

    assert dispatcher(event) is False
    content = event.content
    assert "[CoWorker Desktop 错误]" in content
    assert "read_file" in content
    # inline keeps a prefix of the message but not the whole thing
    assert "boom-" in content
    assert long_message not in content

    path = _detail_path_from(content)
    full = Path(path).read_text(encoding="utf-8")
    assert "[CoWorker Desktop 错误]" in full
    assert long_message in full
    assert len(content) < len(full)


def test_long_askuserquestion_folds_descriptions_to_detail_file(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    long_desc = "选项说明-" * 60  # ~300 chars per option
    questions = [
        {
            "question": f"第 {i + 1} 题：选什么？",
            "options": [
                {"label": f"选项A{i}", "description": long_desc},
                {"label": f"选项B{i}", "description": long_desc},
            ],
        }
        for i in range(4)
    ]
    payload = {
        "actor_id": "claude",
        "request_id": "q-long",
        "session_id": "session-1",
        "tool_name": "AskUserQuestion",
        "input": {"questions": questions},
    }
    event = _event(
        _envelope("desktop.user_input.requested", payload, conversation_id="session-1"),
        conversation_id="session-1",
    )

    assert dispatcher(event) is False
    content = event.content
    # folded: pointer present
    assert "read_file" in content
    # questions, labels and answers template stay inline so the coworker can answer
    assert "第 1 题" in content
    assert "选项A0" in content
    assert "answers" in content
    assert "user_input_request_id" in content
    # verbose descriptions are NOT inline...
    assert long_desc not in content
    # ...only in the detail file
    path = _detail_path_from(content)
    full = Path(path).read_text(encoding="utf-8")
    assert long_desc in full
    assert "第 1 题" in full
    assert len(content) < len(full)


def test_long_thread_event_message_is_folded(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    long_message = "hello-" * 200
    event = _event(
        _envelope("desktop.thread.event", {"message": long_message}, request_id="t-1")
    )

    assert dispatcher(event) is False
    content = event.content
    assert "read_file" in content
    assert long_message not in content
    assert "hello-" in content  # prefix survives
    path = _detail_path_from(content)
    assert Path(path).read_text(encoding="utf-8") == long_message


def test_detail_files_are_pruned_by_age(tmp_path):
    dispatcher = _dispatcher(tmp_path)
    registry = dispatcher._registry
    fresh = registry.write_detail("fresh", "fresh content")
    stale1 = registry.write_detail("stale1", "stale1 content")
    stale2 = registry.write_detail("stale2", "stale2 content")
    old = time.time() - (8 * 24 * 3600)  # past the 7-day retention window
    os.utime(stale1, (old, old))
    os.utime(stale2, (old, old))

    registry._prune_details()

    assert fresh.exists()
    assert not stale1.exists()
    assert not stale2.exists()


def test_detail_files_are_pruned_by_count(tmp_path, monkeypatch):
    monkeypatch.setattr(
        "coworker.channels.desktop.registry._DETAIL_MAX_FILES", 2
    )
    dispatcher = _dispatcher(tmp_path)
    registry = dispatcher._registry
    paths = [registry.write_detail(f"k{i}", f"content {i}") for i in range(4)]

    existing = [path for path in paths if path.exists()]
    assert len(existing) == 2
    # the two newest survive; the oldest are dropped
    assert paths[-1].exists()
    assert paths[-2].exists()
    assert not paths[0].exists()

