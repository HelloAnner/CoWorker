from __future__ import annotations

import pytest
import pytest_asyncio

from coworker.tools.browser_tools import (
    BrowserActionTool,
    BrowserCloseTool,
    BrowserGetContentTool,
    BrowserListSessionsTool,
    BrowserOpenTool,
    BrowserScreenshotTool,
    BrowserSessionStore,
)

# Offline data: URL — no network needed
_PAGE = "data:text/html," + (
    "<html><head><title>Test Page</title></head>"
    "<body>"
    "<h1 id='heading'>Hello World</h1>"
    "<input id='username' type='text' />"
    "<select id='color'>"
    "<option value='red'>Red</option>"
    "<option value='blue'>Blue</option>"
    "</select>"
    "<button id='btn' "
    "onclick=\"document.getElementById('heading').textContent='Clicked'\">Click Me</button>"
    "</body></html>"
)


@pytest_asyncio.fixture(scope="module", loop_scope="module")
async def browser_store():
    """Keep one Playwright process for this module."""
    store = BrowserSessionStore()
    yield store
    await store.stop()


@pytest_asyncio.fixture(loop_scope="module")
async def store(browser_store: BrowserSessionStore):
    """Reuse Playwright while keeping browser sessions isolated per test."""
    yield browser_store
    for session in list(browser_store.all()):
        if session.browser:
            await session.browser.close()
        browser_store.remove(session.session_id)


@pytest.fixture
def screenshot_dir(tmp_path, monkeypatch):
    import coworker.tools.browser_tools as bt
    d = tmp_path / "screenshots"
    d.mkdir()
    monkeypatch.setattr(bt, "_SCREENSHOTS_DIR", d)
    return d


def make_tools(s: BrowserSessionStore):
    return (
        BrowserOpenTool(s),
        BrowserScreenshotTool(s),
        BrowserActionTool(s),
        BrowserGetContentTool(s),
        BrowserCloseTool(s),
        BrowserListSessionsTool(s),
    )


# ---------------------------------------------------------------------------
# Store — pure Python, no browser
# ---------------------------------------------------------------------------

class TestBrowserSessionStore:
    def test_create_returns_unique_ids(self):
        s = BrowserSessionStore()
        ids = {s.create("http://a").session_id for _ in range(10)}
        assert len(ids) == 10

    def test_get_existing_session(self):
        s = BrowserSessionStore()
        session = s.create("http://example.com")
        assert s.get(session.session_id) is session

    def test_get_missing_returns_none(self):
        assert BrowserSessionStore().get("nonexistent") is None

    def test_remove_deletes_session(self):
        s = BrowserSessionStore()
        session = s.create("http://example.com")
        s.remove(session.session_id)
        assert s.get(session.session_id) is None

    def test_remove_nonexistent_is_safe(self):
        BrowserSessionStore().remove("ghost")

    def test_all_returns_all_sessions(self):
        s = BrowserSessionStore()
        a = s.create("http://a")
        b = s.create("http://b")
        assert {x.session_id for x in s.all()} == {a.session_id, b.session_id}


# ---------------------------------------------------------------------------
# BrowserOpenTool
# ---------------------------------------------------------------------------

@pytest.mark.asyncio(loop_scope="module")
class TestBrowserOpenTool:
    async def test_open_registers_session(self, store: BrowserSessionStore):
        open_t, _, _, _, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        assert not r.is_error
        sid = r.content.split("session_id=")[1].split("\n")[0]
        assert store.get(sid) is not None
        await close_t.execute(session_id=sid)

    async def test_open_returns_title(self, store: BrowserSessionStore):
        open_t, _, _, _, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        assert "Test Page" in r.content
        sid = r.content.split("session_id=")[1].split("\n")[0]
        await close_t.execute(session_id=sid)

    async def test_open_defaults_locale_to_zh_cn(self, store: BrowserSessionStore):
        open_t, _, _, _, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        assert not r.is_error
        sid = r.content.split("session_id=")[1].split("\n")[0]
        language = await store.get(sid).page.evaluate("navigator.language")
        assert language == "zh-CN"
        await close_t.execute(session_id=sid)

    async def test_open_allows_locale_override(self, store: BrowserSessionStore):
        open_t, _, _, _, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE, locale="en-US")
        assert not r.is_error
        sid = r.content.split("session_id=")[1].split("\n")[0]
        language = await store.get(sid).page.evaluate("navigator.language")
        assert language == "en-US"
        await close_t.execute(session_id=sid)

    async def test_invalid_url_returns_error(self, store: BrowserSessionStore):
        open_t, *_ = make_tools(store)
        r = await open_t.execute(url="not-valid://??!!")
        assert r.is_error

    async def test_playwright_instance_is_shared(self, store: BrowserSessionStore):
        open_t, _, _, _, close_t, _ = make_tools(store)
        r1 = await open_t.execute(url=_PAGE)
        r2 = await open_t.execute(url=_PAGE)
        assert store._playwright is not None
        sid1 = r1.content.split("session_id=")[1].split("\n")[0]
        sid2 = r2.content.split("session_id=")[1].split("\n")[0]
        assert store.get(sid1).browser is not store.get(sid2).browser
        await close_t.execute(session_id=sid1)
        await close_t.execute(session_id=sid2)


# ---------------------------------------------------------------------------
# BrowserScreenshotTool
# ---------------------------------------------------------------------------

@pytest.mark.asyncio(loop_scope="module")
class TestBrowserScreenshotTool:
    async def test_screenshot_creates_file(self, store: BrowserSessionStore, screenshot_dir):
        open_t, shot_t, _, _, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        result = await shot_t.execute(session_id=sid)
        assert not result.is_error
        import pathlib
        assert pathlib.Path(result.content.split("：")[1].strip()).exists()
        await close_t.execute(session_id=sid)

    async def test_screenshot_counter_increments(self, store: BrowserSessionStore, screenshot_dir):
        open_t, shot_t, _, _, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        await shot_t.execute(session_id=sid)
        await shot_t.execute(session_id=sid)
        assert store.get(sid).screenshot_count == 2
        await close_t.execute(session_id=sid)

    async def test_screenshot_missing_session_returns_error(
        self, store: BrowserSessionStore, screenshot_dir,
    ):
        _, shot_t, *_ = make_tools(store)
        r = await shot_t.execute(session_id="ghost")
        assert r.is_error
        assert "not found" in r.content


# ---------------------------------------------------------------------------
# BrowserActionTool
# ---------------------------------------------------------------------------

@pytest.mark.asyncio(loop_scope="module")
class TestBrowserActionTool:
    async def test_click_action(self, store: BrowserSessionStore):
        open_t, _, action_t, content_t, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        r = await action_t.execute(session_id=sid, action="click", selector="#btn")
        assert not r.is_error
        content = await content_t.execute(session_id=sid, fmt="text", selector="#heading")
        assert "Clicked" in content.content
        await close_t.execute(session_id=sid)

    async def test_type_action(self, store: BrowserSessionStore):
        open_t, _, action_t, _, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        await action_t.execute(session_id=sid, action="type", selector="#username", value="hello")
        val = await store.get(sid).page.locator("#username").input_value()
        assert val == "hello"
        await close_t.execute(session_id=sid)

    async def test_select_action(self, store: BrowserSessionStore):
        open_t, _, action_t, _, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        r = await action_t.execute(session_id=sid, action="select", selector="#color", value="blue")
        assert not r.is_error
        await close_t.execute(session_id=sid)

    async def test_navigate_action_updates_url(self, store: BrowserSessionStore):
        open_t, _, action_t, _, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        new_page = "data:text/html,<title>Second</title><body>Second Page</body>"
        r = await action_t.execute(session_id=sid, action="navigate", selector=new_page)
        assert not r.is_error
        assert store.get(sid).url == new_page
        await close_t.execute(session_id=sid)

    async def test_action_missing_session_returns_error(self, store: BrowserSessionStore):
        _, _, action_t, *_ = make_tools(store)
        r = await action_t.execute(session_id="ghost", action="click", selector="#btn")
        assert r.is_error
        assert "not found" in r.content


# ---------------------------------------------------------------------------
# BrowserGetContentTool
# ---------------------------------------------------------------------------

@pytest.mark.asyncio(loop_scope="module")
class TestBrowserGetContentTool:
    async def test_get_text_content(self, store: BrowserSessionStore):
        open_t, _, _, content_t, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        r = await content_t.execute(session_id=sid, fmt="text")
        assert not r.is_error
        assert "Hello World" in r.content
        await close_t.execute(session_id=sid)

    async def test_get_html_content(self, store: BrowserSessionStore):
        open_t, _, _, content_t, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        r = await content_t.execute(session_id=sid, fmt="html")
        assert not r.is_error
        assert "<h1" in r.content
        await close_t.execute(session_id=sid)

    async def test_get_content_with_selector(self, store: BrowserSessionStore):
        open_t, _, _, content_t, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        r = await content_t.execute(session_id=sid, fmt="text", selector="#heading")
        assert "Hello World" in r.content
        await close_t.execute(session_id=sid)

    async def test_get_content_missing_session_returns_error(self, store: BrowserSessionStore):
        _, _, _, content_t, *_ = make_tools(store)
        r = await content_t.execute(session_id="ghost")
        assert r.is_error

    async def test_get_content_truncation_notice(self, store: BrowserSessionStore):
        open_t, _, _, content_t, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        # 模拟超长内容：直接调用 execute，用 monkeypatch 替换 page.inner_text
        session = store.get(sid)
        long_text = "x" * 8000
        original_inner_text = session.page.inner_text

        async def fake_inner_text(_selector):
            return long_text

        session.page.inner_text = fake_inner_text

        r = await content_t.execute(session_id=sid)
        assert not r.is_error
        assert "内容已截断" in r.content
        assert "start=3000" in r.content
        assert len(r.content) > 3000  # 含截断提示

        session.page.inner_text = original_inner_text
        await close_t.execute(session_id=sid)

    async def test_get_content_pagination(self, store: BrowserSessionStore):
        open_t, _, _, content_t, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        session = store.get(sid)
        long_text = "a" * 5000

        async def fake_inner_text(_selector):
            return long_text

        session.page.inner_text = fake_inner_text

        r1 = await content_t.execute(session_id=sid, start=0)
        assert not r1.is_error
        assert "start=3000" in r1.content

        r2 = await content_t.execute(session_id=sid, start=3000)
        assert not r2.is_error
        assert "内容已截断" not in r2.content  # 第二页已到末尾，无需截断提示
        assert r2.content.startswith("a" * 2000)

        await close_t.execute(session_id=sid)


# ---------------------------------------------------------------------------
# BrowserCloseTool
# ---------------------------------------------------------------------------

@pytest.mark.asyncio(loop_scope="module")
class TestBrowserCloseTool:
    async def test_close_returns_page_titles_and_urls(self, store: BrowserSessionStore):
        open_t, _, _, _, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]
        second_url = "data:text/html,<title>Second Page</title>"
        await store.get(sid).context.new_page()
        await store.get(sid).context.pages[-1].goto(second_url)

        r = await close_t.execute(session_id=sid)

        assert "title=Test Page" in r.content
        assert f"url={_PAGE}" in r.content
        assert "title=Second Page" in r.content
        assert f"url={second_url}" in r.content

    async def test_close_removes_session(self, store: BrowserSessionStore):
        open_t, _, _, _, close_t, _ = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        r = await close_t.execute(session_id=sid)
        assert not r.is_error
        assert store.get(sid) is None

    async def test_close_missing_session_returns_error(self, store: BrowserSessionStore):
        _, _, _, _, close_t, _ = make_tools(store)
        r = await close_t.execute(session_id="ghost")
        assert r.is_error
        assert "not found" in r.content


# ---------------------------------------------------------------------------
# BrowserListSessionsTool
# ---------------------------------------------------------------------------

@pytest.mark.asyncio(loop_scope="module")
class TestBrowserListSessionsTool:
    async def test_empty_store(self, store: BrowserSessionStore):
        _, _, _, _, _, list_t = make_tools(store)
        r = await list_t.execute()
        assert not r.is_error
        assert "没有" in r.content

    async def test_lists_open_sessions(self, store: BrowserSessionStore):
        open_t, _, _, _, close_t, list_t = make_tools(store)
        r = await open_t.execute(url=_PAGE)
        sid = r.content.split("session_id=")[1].split("\n")[0]

        r = await list_t.execute()
        assert sid in r.content
        await close_t.execute(session_id=sid)


# ---------------------------------------------------------------------------
# End-to-end flow
# ---------------------------------------------------------------------------

@pytest.mark.asyncio(loop_scope="module")
class TestEndToEndFlow:
    async def test_open_interact_extract_close(self, store: BrowserSessionStore, screenshot_dir):
        open_t, shot_t, action_t, content_t, close_t, list_t = make_tools(store)

        r = await open_t.execute(url=_PAGE)
        assert not r.is_error
        sid = r.content.split("session_id=")[1].split("\n")[0]

        sessions = await list_t.execute()
        assert sid in sessions.content

        shot = await shot_t.execute(session_id=sid)
        assert not shot.is_error

        await action_t.execute(session_id=sid, action="type", selector="#username", value="tester")
        await action_t.execute(session_id=sid, action="select", selector="#color", value="blue")

        text = await content_t.execute(session_id=sid, fmt="text")
        assert "Hello World" in text.content

        r = await close_t.execute(session_id=sid)
        assert not r.is_error
        assert store.get(sid) is None

        sessions = await list_t.execute()
        assert sid not in sessions.content
