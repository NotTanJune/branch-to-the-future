from __future__ import annotations

import importlib
from types import ModuleType


class MissingExtraError(RuntimeError):
    """Raised when an optional brf-agent feature is used without its extra installed."""


def require(module: str, extra: str) -> ModuleType:
    """Import an optional dependency or explain which extra provides it."""
    try:
        return importlib.import_module(module)
    except ImportError as exc:
        raise MissingExtraError(
            f"this command needs the optional '{extra}' dependencies; "
            f"install them with: uv pip install 'brf-agent[{extra}]' "
            f"(or 'brf-agent[all]')"
        ) from exc
