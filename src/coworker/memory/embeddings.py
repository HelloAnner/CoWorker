from __future__ import annotations

from typing import Any, Protocol, cast


class EmbeddingFunction(Protocol):
    def __call__(self, texts: list[str]) -> list[list[float]]:
        ...


def make_embedding_function(provider: str = "local") -> EmbeddingFunction:
    if provider == "local":
        return _LocalEmbedding()
    raise ValueError(f"Unknown embedding provider: {provider}")


class _LocalEmbedding:
    def __init__(self) -> None:
        self._model: Any | None = None

    def _load(self) -> None:
        if self._model is None:
            from sentence_transformers import SentenceTransformer
            self._model = SentenceTransformer("all-MiniLM-L6-v2")

    def __call__(self, texts: list[str]) -> list[list[float]]:
        self._load()
        model = self._model
        if model is None:
            raise RuntimeError("Embedding model failed to load")
        return cast(list[list[float]], model.encode(texts, convert_to_numpy=True).tolist())
