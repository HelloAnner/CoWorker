from coworker.brain.tls import shared_ssl_context


def test_shared_ssl_context_is_built_once(monkeypatch):
    context = object()
    calls = 0

    def create_context():
        nonlocal calls
        calls += 1
        return context

    shared_ssl_context.cache_clear()
    monkeypatch.setattr("coworker.brain.tls.httpx.create_ssl_context", create_context)
    try:
        assert shared_ssl_context() is context
        assert shared_ssl_context() is context
        assert calls == 1
    finally:
        shared_ssl_context.cache_clear()
