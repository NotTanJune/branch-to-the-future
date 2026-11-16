from __future__ import annotations

import os
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field

from .context import load_file_context, resolve_repo_root
from .patch import draft_patch_plan, draft_unified_diff_draft
from .reports import render_markdown_report
from .transport import BrfTransport


class AgenticAnalysis(BaseModel):
    model_config = ConfigDict(extra="forbid")

    markdown_report: str
    patch_plan: list[str] = Field(default_factory=list)
    unified_diff_draft: str | None = None
    json_envelope: dict


def create_impact_orchestrator(
    transport: BrfTransport | None = None, root: str | Path | None = None
):
    from .extras import require

    agents_sdk = require("agents", "agents")
    Agent, function_tool = agents_sdk.Agent, agents_sdk.function_tool

    transport = transport or BrfTransport()
    allowed = resolve_repo_root(root or os.environ.get("BRF_AGENT_ROOT") or os.getcwd())

    def check_repo(repo: str, *, allow_remote: bool = False) -> str:
        if allow_remote and repo.startswith(("https://github.com/", "http://github.com/")):
            return repo
        return str(resolve_repo_root(repo, allowed))

    @function_tool
    def run_rust_scan_json(repo: str) -> dict:
        """Run `brf scan --json` and return the versioned envelope."""
        return transport.scan(check_repo(repo, allow_remote=True)).model_dump(mode="json")

    @function_tool
    def run_rust_analyze_json(repo: str, change: str) -> dict:
        """Run `brf analyze --json` and return the versioned envelope."""
        return transport.analyze(check_repo(repo, allow_remote=True), change).model_dump(mode="json")

    @function_tool
    def load_bounded_file_context(repo: str, paths: list[str]) -> dict:
        """Read bounded, read-only file context for repo-relative paths."""
        return load_file_context(check_repo(repo), paths, allowed_root=allowed).model_dump(mode="json")

    @function_tool(strict_mode=False)
    def render_report(envelope: dict) -> str:
        """Render a Markdown report from a versioned envelope."""
        from .schemas import JsonEnvelope

        return render_markdown_report(JsonEnvelope.model_validate(envelope))

    @function_tool(strict_mode=False)
    def draft_patch_plan_tool(envelope: dict) -> dict:
        """Draft a patch plan without mutating files."""
        from .schemas import JsonEnvelope

        return draft_patch_plan(JsonEnvelope.model_validate(envelope)).model_dump(mode="json")

    instructions = """
You are Impact Orchestrator for Branch to the Future.
Use deterministic tools first. Return structured output only.
Do not claim to edit files. Do not write files. Draft unified diffs only after bounded file context is loaded.
Reject paths outside the repo root by relying on the file context tool.
Keep recommendations concise and testable.
""".strip()

    return Agent(
        name="Impact Orchestrator",
        instructions=instructions,
        model=os.environ.get("BRF_AGENT_MODEL", "gpt-5-mini"),
        tools=[
            run_rust_scan_json,
            run_rust_analyze_json,
            load_bounded_file_context,
            render_report,
            draft_patch_plan_tool,
        ],
        output_type=AgenticAnalysis,
    )


async def run_agentic_turn(
    prompt: str,
    *,
    session_id: str | None = None,
    session_db: str | Path | None = None,
    transport: BrfTransport | None = None,
) -> AgenticAnalysis | str:
    from .extras import require

    agents_sdk = require("agents", "agents")
    Runner, SQLiteSession = agents_sdk.Runner, agents_sdk.SQLiteSession

    agent = create_impact_orchestrator(transport=transport)
    session = None
    if session_id is not None:
        db_path = Path(session_db) if session_db is not None else _default_session_db()
        db_path.parent.mkdir(parents=True, exist_ok=True)
        session = SQLiteSession(session_id, str(db_path))
    result = await Runner.run(agent, prompt, session=session)
    return result.final_output


def draft_diff_after_context(envelope, context) -> str:
    return draft_unified_diff_draft(envelope, context=context)


def _default_session_db() -> Path:
    return Path(os.environ.get("BRF_AGENT_SESSION_DB", ".brf-agent/sessions.sqlite"))
