from functools import cache
from ssl import SSLContext

import httpx


@cache
def shared_ssl_context() -> SSLContext:
    """Build the process-wide CA context once; HTTP clients keep separate pools."""
    return httpx.create_ssl_context()
