import json
import sys
from pathlib import Path

import pytest

from brf_agent.cli import main
from brf_agent.agent import create_impact_orchestrator
from brf_agent.context import load_file_context
from brf_agent.patch import draft_patch_plan, draft_unified_diff_draft
from brf_agent.reports import render_markdown_report
from brf_agent.schemas import JsonEnvelope, parse_envelope_json
from brf_agent.transport import BrfTransport, BrfTransportError


FIXTURES = Path(__file__).resolve().parents[2] / "fixtures" / "schema"


def test_pydantic_parses_rust_fixtures() -> None:
    scan = JsonEnvelope.model_validate_json(
        (FIXTURES / "scan-envelope-v1.json").read_text()
    )
    analyze = JsonEnvelope.model_validate_json(
        (FIXTURES / "analyze-envelope-v1.json").read_text()
    )

    assert scan.schema_version == 1
    assert scan.impact_analysis is None
    assert analyze.impact_analysis is not None
    assert analyze.impact_analysis.impact_path[0].impact_score == 90


def test_schema_version_mismatch_fails_clearly() -> None:
    data = json.loads((FIXTURES / "scan-envelope-v1.json").read_text())
    data["schema_version"] = 99

    with pytest.raises(ValueError, match="schema_version 99 is not supported"):
        parse_envelope_json(json.dumps(data))


def test_invalid_json_fails_clearly() -> None:
    with pytest.raises(ValueError, match="invalid JSON from brf"):
        parse_envelope_json("{not json")


def test_fake_brf_transport_scan_and_analyze(tmp_path: Path) -> None:
    fake = tmp_path / "fake_brf.py"
    fake.write_text(
        """
import pathlib
import sys

root = pathlib.Path(sys.argv[1]).resolve()
fixture_root = root / "fixtures" / "schema"
if sys.argv[2] == "scan":
    print((fixture_root / "scan-envelope-v1.json").read_text())
elif sys.argv[2] == "analyze":
    print((fixture_root / "analyze-envelope-v1.json").read_text())
else:
    raise SystemExit(2)
""".strip()
    )
    transport = BrfTransport(command=[sys.executable, str(fake), str(Path.cwd())])

    scan = transport.scan("demo")
    analyze = transport.analyze("demo", "add async resume parsing")

    assert scan.impact_analysis is None
    assert analyze.change_request == "add async resume parsing"


def test_transport_reports_invalid_json(tmp_path: Path) -> None:
    fake = tmp_path / "fake_brf.py"
    fake.write_text("print('{bad json')\n")
    transport = BrfTransport(command=[sys.executable, str(fake)])

    with pytest.raises(BrfTransportError, match="invalid JSON from brf"):
        transport.scan("demo")


def test_file_context_refuses_outside_root(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    outside = tmp_path / "outside.txt"
    outside.write_text("secret")

    with pytest.raises(ValueError, match="outside repo root"):
        load_file_context(repo, ["../outside.txt"])


def test_file_context_redacts_secret_like_values(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    source = repo / "app.py"
    source.write_text("OPENAI_API_KEY = 'sk-test-secret-value'\n")

    context = load_file_context(repo, ["app.py"], max_file_bytes=1000)

    assert "sk-test-secret-value" not in context.files[0].content
    assert "[REDACTED]" in context.files[0].content
    assert source.read_text() == "OPENAI_API_KEY = 'sk-test-secret-value'\n"


def test_patch_draft_requires_loaded_context_for_diff() -> None:
    envelope = JsonEnvelope.model_validate_json(
        (FIXTURES / "analyze-envelope-v1.json").read_text()
    )

    plan = draft_patch_plan(envelope)
    assert plan.steps

    with pytest.raises(ValueError, match="file context is required"):
        draft_unified_diff_draft(envelope, context=None)


def test_markdown_report_includes_architecture_evidence_and_appendix() -> None:
    envelope = JsonEnvelope.model_validate_json(
        (FIXTURES / "analyze-envelope-v1.json").read_text()
    )

    report = render_markdown_report(envelope)

    assert "## Current Architecture" in report
    assert "## Proposed Architecture" in report
    assert "## Evidence" in report
    assert "## JSON Appendix" in report


def test_cli_analyze_renders_human_report_with_fake_brf(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    fake = tmp_path / "brf"
    fake.write_text(
        f"""#!{sys.executable}
from pathlib import Path

print((Path({str(FIXTURES)!r}) / "analyze-envelope-v1.json").read_text())
"""
    )
    fake.chmod(0o755)

    exit_code = main(
        [
            "analyze",
            "demo",
            "--change",
            "add async resume parsing",
            "--brf-binary",
            str(fake),
        ]
    )
    out = capsys.readouterr().out

    assert exit_code == 0
    assert "# Branch to the Future Agent Report" in out
    assert "Minimal Queue Handoff" in out


def test_agents_sdk_orchestrator_constructs_without_api_call() -> None:
    agent = create_impact_orchestrator(transport=BrfTransport(command=["brf"]))

    assert agent.name == "Impact Orchestrator"
