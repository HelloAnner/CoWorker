FROM python:3.13-bookworm

# Install system deps + Node.js 24 via NodeSource
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates gnupg ffmpeg \
    && mkdir -p /etc/apt/keyrings \
    && curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
       | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg \
    && echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_24.x nodistro main" \
       > /etc/apt/sources.list.d/nodesource.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

COPY --from=ghcr.io/astral-sh/uv:latest /uv /usr/local/bin/uv

WORKDIR /app

ENV UV_PROJECT_ENVIRONMENT=/opt/venv \
    PLAYWRIGHT_BROWSERS_PATH=/opt/ms-playwright

# Install dependencies only (cached unless pyproject.toml or uv.lock changes)
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-install-project

# Install Playwright Chromium + system dependencies (cached, runs before source copy)
RUN uv run playwright install --with-deps chromium

# Copy source and install local package (fast, deps already cached)
COPY . .
RUN uv sync --frozen --dev

# Create runtime data directories (override by volume mount in production)
RUN mkdir -p data/inbox data/outbox data/identity data/logs data/memory data/workspace .coworker/skills

EXPOSE 8000

CMD ["uv", "run", "coworker"]
