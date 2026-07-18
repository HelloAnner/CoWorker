"""Token count estimation utilities.

Ported from https://github.com/johannschopplich/tokenx (MIT).
Segment-based heuristics: no tokenizer dependency, ~95-98% accuracy.
"""
from __future__ import annotations

import json
import math
import re
from typing import Any

_PUNCT_PATTERN = r'[.,!?;(){}\[\]<>:\/\\|@#$%^&*+=`~_\-]'
_TOKEN_SPLIT = re.compile(r'(\s+|' + _PUNCT_PATTERN + r'+)')

_CJK = re.compile(
    r"[\u4E00-\u9FFF\u3400-\u4DBF\u3000-\u303F\uFF00-\uFFEF\u30A0-\u30FF\u2E80-\u2EFF\u31C0-\u31EF\u3200-\u32FF\u3300-\u33FF\uAC00-\uD7AF\u1100-\u11FF\u3130-\u318F\uA960-\uA97F\uD7B0-\uD7FF]"
)
_NUMERIC = re.compile(r'^\d+(?:[.,]\d+)*$')
_PUNCT_ONLY = re.compile(r'^' + _PUNCT_PATTERN + r'+$')
_WHITESPACE = re.compile(r'^\s+$')

_DEFAULT_CHARS_PER_TOKEN = 6
_SHORT_TOKEN_THRESHOLD = 3

_LANGUAGE_CONFIGS: list[tuple[re.Pattern[str], float]] = [
    (re.compile(r'[äöüßẞ]', re.IGNORECASE), 3.0),
    (re.compile(r'[éèêëàâîïôûùüÿçœæáíóúñ]', re.IGNORECASE), 3.0),
    (re.compile(r'[ąćęłńóśźżěščřžýůúďťň]', re.IGNORECASE), 3.5),
]

# Fixed estimate for image blocks (Anthropic: 85 base + tiles; mid-range for typical photos).
IMAGE_TOKEN_ESTIMATE = 1000
# Fallback when no PDF data is available to size from.
DOCUMENT_TOKEN_ESTIMATE = 2500


def _lang_chars_per_token(segment: str) -> float:
    for pattern, ratio in _LANGUAGE_CONFIGS:
        if pattern.search(segment):
            return ratio
    return _DEFAULT_CHARS_PER_TOKEN


def _segment_tokens(segment: str) -> int:
    if not segment or _WHITESPACE.match(segment):
        return 0
    if _CJK.search(segment):
        return len(segment)
    if _NUMERIC.match(segment):
        return 1
    if len(segment) <= _SHORT_TOKEN_THRESHOLD:
        return 1
    if _PUNCT_ONLY.match(segment):
        return math.ceil(len(segment) / 2)
    return math.ceil(len(segment) / _lang_chars_per_token(segment))


def estimate_text_tokens(text: str) -> int:
    """Estimate token count for a plain-text string.

    CJK chars → 1 token each; numbers → 1 token; short words → 1 token;
    punctuation sequences → ceil(n/2); other words → ceil(len / chars_per_token)
    with language-specific ratios for European scripts.
    """
    if not text:
        return 0
    return sum(_segment_tokens(s) for s in _TOKEN_SPLIT.split(text) if s)


def _estimate_document_tokens(block: dict[str, Any]) -> int:
    """Estimate tokens for a PDF document block.

    When base64 data is present: binary_bytes ≈ base64_len × 3/4;
    Anthropic charges ~2500 tokens/page at ~100 KB/page → 1 token per 40 bytes.
    Falls back to DOCUMENT_TOKEN_ESTIMATE (≈1 page) when data is absent.
    """
    data: str = block.get("source", {}).get("data", "")
    if not data:
        return DOCUMENT_TOKEN_ESTIMATE
    binary_bytes = len(data) * 3 // 4
    return max(DOCUMENT_TOKEN_ESTIMATE, binary_bytes // 40)


def estimate_content_tokens(content: str | list[dict[str, Any]]) -> int:
    """Estimate token count for a message content value (Anthropic-style blocks).

    Provider-agnostic fallback. For provider-aware estimation use
    BaseLLMProvider.estimate_content_tokens, which accounts for vision capability
    and format conversion before counting.

    - image / image_url blocks → IMAGE_TOKEN_ESTIMATE (base64 length is irrelevant)
    - document blocks → size-based estimate from PDF data, or DOCUMENT_TOKEN_ESTIMATE
    - text blocks → estimate_text_tokens()
    - other blocks → estimate_text_tokens() on JSON of non-binary fields
    """
    if isinstance(content, str):
        return estimate_text_tokens(content)
    total = 0
    for block in content:
        btype = block.get("type")
        if btype in ("image", "image_url"):
            total += IMAGE_TOKEN_ESTIMATE
        elif btype == "document":
            total += _estimate_document_tokens(block)
        elif btype == "text":
            total += estimate_text_tokens(block.get("text", ""))
        else:
            safe = {k: v for k, v in block.items() if k not in ("data", "source")}
            total += estimate_text_tokens(json.dumps(safe))
    return total
