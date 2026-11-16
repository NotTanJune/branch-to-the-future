from __future__ import annotations

from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field

from .redact import scrub_secrets


class FileContext(BaseModel):
    model_config = ConfigDict(extra="forbid")

    path: str
    content: str
    bytes_read: int = Field(ge=0)
    truncated: bool


class FileContextBundle(BaseModel):
    model_config = ConfigDict(extra="forbid")

    repo_root: str
    files: list[FileContext]
    total_bytes: int = Field(ge=0)


SENSITIVE_DIRS = {".git"}


def is_sensitive_path(rel: Path) -> bool:
    if any(part in SENSITIVE_DIRS for part in rel.parts):
        return True
    name = rel.name.lower()
    return (
        name == ".env"
        or name.startswith(".env.")
        or name.endswith((".pem", ".key"))
        or name.startswith("id_rsa")
    )


def resolve_repo_root(repo: str | Path, allowed_root: str | Path | None = None) -> Path:
    root = Path(repo).resolve()
    if root == Path(root.anchor):
        raise ValueError("refusing to use the filesystem root as a repo")
    if allowed_root is not None:
        allowed = Path(allowed_root).resolve()
        try:
            root.relative_to(allowed)
        except ValueError as exc:
            raise ValueError(f"repo is outside the allowed root: {repo}") from exc
    return root


def load_file_context(
    repo_root: str | Path,
    paths: list[str],
    *,
    max_file_bytes: int = 20_000,
    max_total_bytes: int = 60_000,
    allowed_root: str | Path | None = None,
) -> FileContextBundle:
    root = resolve_repo_root(repo_root, allowed_root)
    files: list[FileContext] = []
    total = 0

    for rel_path in paths:
        target = _resolve_inside(root, rel_path)
        if not target.is_file():
            raise ValueError(f"file does not exist: {rel_path}")
        remaining = max_total_bytes - total
        if remaining <= 0:
            break
        limit = min(max_file_bytes, remaining)
        raw = target.read_bytes()
        truncated = len(raw) > limit
        raw = raw[:limit]
        content = raw.decode("utf-8", errors="replace")
        content = scrub_secrets(content)
        total += len(raw)
        files.append(
            FileContext(
                path=target.relative_to(root).as_posix(),
                content=content,
                bytes_read=len(raw),
                truncated=truncated,
            )
        )

    return FileContextBundle(repo_root=str(root), files=files, total_bytes=total)


def _resolve_inside(root: Path, rel_path: str) -> Path:
    target = (root / rel_path).resolve()
    try:
        target.relative_to(root)
    except ValueError as exc:
        raise ValueError(f"path is outside repo root: {rel_path}") from exc
    if is_sensitive_path(target.relative_to(root)):
        raise ValueError(f"refusing to read sensitive file: {rel_path}")
    return target
