from __future__ import annotations

from pydantic import BaseModel, ConfigDict

from .context import FileContextBundle
from .schemas import ImplementationFuture, JsonEnvelope


class PatchPlan(BaseModel):
    model_config = ConfigDict(extra="forbid")

    files: list[str]
    steps: list[str]


def draft_patch_plan(envelope: JsonEnvelope) -> PatchPlan:
    analysis = envelope.impact_analysis
    if analysis is None:
        return PatchPlan(files=[], steps=["Run analyze before drafting a patch plan."])

    future = _selected_future(envelope)
    if future is not None:
        files = future.affected_files
        steps = future.patch_plan or [
            f"Inspect and update {path} for {future.name}." for path in files
        ]
        return PatchPlan(files=files, steps=steps)

    files = [item.path for item in analysis.impact_path]
    steps = [f"{item.path}: {item.change_needed}" for item in analysis.impact_path]
    return PatchPlan(files=files, steps=steps)


def draft_unified_diff_draft(
    envelope: JsonEnvelope, *, context: FileContextBundle | None
) -> str:
    if context is None or not context.files:
        raise ValueError("file context is required before drafting a unified diff")

    plan = draft_patch_plan(envelope)
    step_text = "; ".join(plan.steps[:3]) or "Apply selected patch plan."
    chunks: list[str] = []
    for file_context in context.files:
        chunks.append(
            "\n".join(
                [
                    f"diff --git a/{file_context.path} b/{file_context.path}",
                    f"--- a/{file_context.path}",
                    f"+++ b/{file_context.path}",
                    "@@",
                    f"+# Draft only: {step_text}",
                ]
            )
        )
    return "\n".join(chunks) + "\n"


def _selected_future(envelope: JsonEnvelope) -> ImplementationFuture | None:
    analysis = envelope.impact_analysis
    if analysis is None:
        return None
    for future in analysis.futures:
        if future.name == analysis.recommended_future:
            return future
    return analysis.futures[0] if analysis.futures else None
