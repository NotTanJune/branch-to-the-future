from __future__ import annotations

import re

REDACTED = "[REDACTED]"

_PEM = re.compile(
    r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----.*?(?:-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----|\Z)",
    re.DOTALL,
)
_TOKEN_SHAPES = re.compile(
    r"\bAKIA[0-9A-Z]{16}\b"
    r"|\bgithub_pat_[A-Za-z0-9_]+"
    r"|\bgh[pousr]_[A-Za-z0-9]{3,}"
    r"|\bsk-[A-Za-z0-9_\-]{3,}"
)
_URL_CREDS = re.compile(r"(\b[A-Za-z][A-Za-z0-9+.\-]*://[^\s/:@]*:)[^\s/@]+(@)")
_AUTHORIZATION = re.compile(
    r"(?i)(\bauthorization[\"']?\s*[:=]\s*[\"']?)(?:(?:bearer|basic|token)\s+)?[^\s\"',;]+"
)
_BEARER = re.compile(r"(?i)(\bbearer\s+)[A-Za-z0-9._~+/=\-]+")
_KEY_VALUE = re.compile(
    r"(?i)([\w.\-]*(?:api[_\-]?key|token|secret|passw(?:or)?d|pwd|private[_\-]?key|credential)[\w.\-]*"
    r"[\"'\]]*\s*[:=]\s*)(?:\"[^\"\n]*\"|'[^'\n]*'|[^\s,;]+)"
)


def scrub_secrets(text: str) -> str:
    """Redact secret-like values from text. Shared by file context and transport errors."""
    text = _PEM.sub(REDACTED, text)
    text = _TOKEN_SHAPES.sub(REDACTED, text)
    text = _URL_CREDS.sub(rf"\1{REDACTED}\2", text)
    text = _AUTHORIZATION.sub(rf"\1{REDACTED}", text)
    text = _BEARER.sub(rf"\1{REDACTED}", text)
    text = _KEY_VALUE.sub(rf"\1{REDACTED}", text)
    return text
