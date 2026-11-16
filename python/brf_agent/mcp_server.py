from __future__ import annotations

import os
from pathlib import Path

from .context import load_file_context, resolve_repo_root
from .patch import draft_patch_plan
from .reports import render_markdown_report as render_report
from .schemas import JsonEnvelope
from .transport import BrfTransport


def pinned_root(root: str | Path | None = None) -> Path:
    """Allowed root: explicit value, then BRF_AGENT_ROOT, then the process cwd."""
    chosen = root or os.environ.get("BRF_AGENT_ROOT") or os.getcwd()
    return resolve_repo_root(chosen)


def is_remote_repo(repo: str) -> bool:
    return repo.startswith(("https://github.com/", "http://github.com/"))


def run_mcp(root: str | Path | None = None) -> None:
    from .extras import require

    fastmcp = require("mcp.server.fastmcp", "mcp")
    FastMCP = fastmcp.FastMCP
    ToolError = require("mcp.server.fastmcp.exceptions", "mcp").ToolError

    allowed = pinned_root(root)

    def check_repo(repo: str, *, allow_remote: bool = False) -> str:
        if allow_remote and is_remote_repo(repo):
            return repo
        try:
            return str(resolve_repo_root(repo, allowed))
        except ValueError as exc:
            raise ToolError(str(exc)) from exc

    mcp = FastMCP("branch-to-the-future")
    transport = BrfTransport()

    @mcp.tool()
    def scan_repo(repo: str) -> dict:
        """Run brf scan --json for a repository."""
        return transport.scan(check_repo(repo, allow_remote=True)).model_dump(mode="json")

    @mcp.tool()
    def analyze_change(repo: str, change: str) -> dict:
        """Run brf analyze --json for a repository and change request."""
        return transport.analyze(check_repo(repo, allow_remote=True), change).model_dump(mode="json")

    @mcp.tool()
    def load_context(repo: str, paths: list[str]) -> dict:
        """Load bounded read-only file context for repo-relative paths."""
        return load_file_context(check_repo(repo), paths, allowed_root=allowed).model_dump(mode="json")

    @mcp.tool()
    def render_markdown_report(envelope: dict) -> str:
        """Render a Markdown report from a Branch to the Future envelope."""
        return render_report(JsonEnvelope.model_validate(envelope))

    @mcp.tool()
    def draft_patch_plan_tool(envelope: dict) -> dict:
        """Draft a patch plan without writing files."""
        return draft_patch_plan(JsonEnvelope.model_validate(envelope)).model_dump(mode="json")

    mcp.run(transport="stdio")
