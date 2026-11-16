from __future__ import annotations

from .patch import draft_patch_plan
from .schemas import JsonEnvelope


def render_markdown_report(envelope: JsonEnvelope) -> str:
    lines = ["# Branch to the Future Agent Report", ""]
    lines.extend(
        [
            "## Scan Summary",
            "",
            f"- Repo: {envelope.repo_model.repo_name}",
            f"- Files: {len(envelope.repo_model.files)}",
            f"- Routes: {len(envelope.repo_model.routes)}",
            f"- Tests: {len(envelope.repo_model.tests)}",
            f"- Frameworks: {', '.join(envelope.repo_model.frameworks) or 'none'}",
            "",
        ]
    )
    analysis = envelope.impact_analysis
    if analysis is None:
        lines.extend(["## Impact Analysis", "", "_No impact analysis returned._", ""])
        return "\n".join(lines)

    lines.extend(
        [
            "## Impact Table",
            "",
            "<table><thead><tr><th>File</th><th>Score</th><th>Confidence</th><th>Risk</th></tr></thead><tbody>",
        ]
    )
    for item in analysis.impact_path:
        lines.append(
            f"<tr><td><code>{item.path}</code></td><td>{score_out_of_10(item.impact_score)}/10</td><td>{item.confidence}%</td><td>{item.risk.value}</td></tr>"
        )
    lines.append("</tbody></table>")
    lines.append("")

    lines.extend(["## Risk Table", ""])
    for risk in analysis.risk_summary:
        lines.append(f"- {risk}")
    lines.append("")

    lines.extend(["## Current Architecture", ""])
    _append_architecture(lines, analysis.current_architecture)
    lines.append("")

    lines.extend(["## Proposed Architecture", ""])
    future = next(
        (item for item in analysis.futures if item.name == analysis.recommended_future),
        analysis.futures[0] if analysis.futures else None,
    )
    _append_architecture(lines, future.architecture if future else [])
    lines.append("")

    lines.extend(["## Evidence", ""])
    for item in analysis.impact_path:
        lines.extend(
            [
                f"### `{item.path}`",
                "",
                f"- Reason: {item.reason}",
                f"- Change: {item.change_needed}",
                f"- Confidence: {item.confidence}%",
                "",
            ]
        )

    lines.extend(["## Recommended Path", "", analysis.recommended_future, ""])

    lines.extend(["## Test Plan", ""])
    test_plan = future.test_plan if future else analysis.tests_to_add
    for item in test_plan:
        lines.append(f"- {item}")
    lines.append("")

    patch_plan = draft_patch_plan(envelope)
    lines.extend(["## Patch Plan", ""])
    for step in patch_plan.steps:
        lines.append(f"- {step}")
    lines.append("")

    lines.extend(["## JSON Appendix", "", "```json", envelope.model_dump_json(indent=2), "```"])
    return "\n".join(lines)


# Values up to 10 are already ten-point scores. Larger values are 0-100 and
# round half up to the nearest tenth.
def score_out_of_10(score: int) -> int:
    """Render a 0-100 impact score on a ten-point scale (half rounds up).

    brf stores every score on 0-100; ten-point model output is rescaled when it
    is parsed, so this matches the Rust `impact_score_out_of_10`.
    """
    return max(0, min(10, (score + 5) // 10))


def _append_architecture(lines: list[str], stages) -> None:
    if not stages:
        lines.append("_None returned._")
        return
    for stage in stages:
        lines.extend([f"### {stage.label}", ""])
        if stage.responsibilities:
            lines.append("- Responsibilities:")
            for item in stage.responsibilities:
                lines.append(f"  - {item}")
        if stage.files:
            lines.append("- Files:")
            for path in stage.files:
                lines.append(f"  - `{path}`")
        lines.append("")
