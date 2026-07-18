from __future__ import annotations

from coworker.tools.base import PAGE_CHAR_LIMIT, PAGE_CHAR_MAX, paginate_text


def _body(result: str) -> str:
    """去掉首行分页提示，取实际 chunk。"""
    return result.split("\n", 1)[1] if result.startswith("[") else result


class TestPaginateText:
    def test_short_text_no_notice(self):
        result = paginate_text("hello")
        assert result == "hello"
        assert "字符" not in result

    def test_default_page_caps_output(self):
        text = "x" * (PAGE_CHAR_LIMIT * 2)
        result = paginate_text(text)
        assert _body(result) == "x" * PAGE_CHAR_LIMIT
        assert "如需后续内容" in result
        assert "剩余" in result
        assert f"offset={PAGE_CHAR_LIMIT}" in result

    def test_limit_zero_capped_by_hard_max(self):
        text = "x" * (PAGE_CHAR_MAX * 2)
        result = paginate_text(text, limit=0)
        assert len(_body(result)) == PAGE_CHAR_MAX
        assert "如需后续内容" in result

    def test_large_limit_clamped_to_hard_max(self):
        text = "x" * (PAGE_CHAR_MAX * 2)
        result = paginate_text(text, limit=PAGE_CHAR_MAX * 5)
        assert len(_body(result)) == PAGE_CHAR_MAX

    def test_explicit_small_limit(self):
        result = paginate_text("abcdefghij", limit=4)
        assert _body(result) == "abcd"
        assert "offset=4" in result

    def test_offset_reads_tail(self):
        text = "abcdefghij"
        result = paginate_text(text, offset=8, limit=4)
        assert _body(result) == "ij"
        assert "查看后续内容" not in result  # 已到末尾，无剩余
        assert "已到末尾" in result

    def test_offset_beyond_end_empty_chunk(self):
        result = paginate_text("abc", offset=10)
        assert _body(result) == ""
        assert "[offset=10 超出范围 / 共 3 字符]" in result

    def test_offset_with_remaining_shows_next(self):
        text = "abcdefghij"
        result = paginate_text(text, offset=2, limit=4)
        assert _body(result) == "cdef"
        assert "offset=6" in result

    def test_custom_next_hint_template(self):
        text = "abcdefghij"
        result = paginate_text(
            text,
            limit=4,
            next_hint="用 some_tool(offset={offset}) 继续，剩余 {remaining} 字符",
        )
        assert "some_tool(offset=4)" in result
        assert "剩余 6 字符" in result
