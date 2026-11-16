import asyncio
import json
import sys
from pathlib import Path

import pytest

from brf_agent.context import load_file_context
from brf_agent.reports import score_out_of_10
from brf_agent.transport import BrfTransport, BrfTransportError

SAMPLE = """\
"api_key": "sk-json-secret-1"
os.environ["OPENAI_API_KEY"] = "sk-env-secret-2"
password = "my secret pass phrase"
Authorization: Bearer abcdef-bearer-3
AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE
DATABASE_URL=postgres://user:dbpass5@host/db
-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEAxxxprivbody6
-----END RSA PRIVATE KEY-----
const apiKey = "sk-camel-8";
GITHUB_TOKEN=ghp_abc9xyz
client_secret = 'a b c'
gh = github_pat_11AAAA_bbbbCCCC
plain = sk-bareTokenValue99
"""
LEAKS = [
    "sk-json-secret-1", "sk-env-secret-2", "secret pass", "phrase",
    "abcdef-bearer-3", "AKIAIOSFODNN7EXAMPLE", "dbpass5", "privbody6",
    "sk-camel-8", "ghp_abc9xyz", "a b c", "github_pat_11AAAA", "sk-bareTokenValue99",
]


def _mcp_tools(monkeypatch, **kwargs):
    from mcp.server.fastmcp import FastMCP

    import brf_agent.mcp_server as m

    captured = {}
    monkeypatch.setattr(FastMCP, "run", lambda self, *a, **k: captured.setdefault("mcp", self))
    m.run_mcp(**kwargs)
    return captured["mcp"]


def _call(mcp, name, args):
    return asyncio.run(mcp.call_tool(name, args))


# P1
def test_mcp_load_context_refuses_repo_outside_pinned_root(tmp_path, monkeypatch):
    root = tmp_path / "root"
    root.mkdir()
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "hosts").write_text("TOPSECRETHOSTS")
    (root / "ok.txt").write_text("fine")
    mcp = _mcp_tools(monkeypatch, root=str(root))

    with pytest.raises(Exception, match="outside"):
        _call(mcp, "load_context", {"repo": str(outside), "paths": ["hosts"]})
    with pytest.raises(Exception):
        _call(mcp, "load_context", {"repo": "/", "paths": ["etc/hosts"]})
    with pytest.raises(Exception, match="outside"):
        _call(mcp, "scan_repo", {"repo": str(outside)})
    out = _call(mcp, "load_context", {"repo": str(root), "paths": ["ok.txt"]})
    assert "fine" in str(out)


def test_mcp_root_defaults_to_cwd_and_env(tmp_path, monkeypatch):
    (tmp_path / "a").mkdir()
    (tmp_path / "b").mkdir()
    (tmp_path / "a" / "f.txt").write_text("A")
    (tmp_path / "b" / "f.txt").write_text("B")
    monkeypatch.chdir(tmp_path / "a")
    monkeypatch.delenv("BRF_AGENT_ROOT", raising=False)
    mcp = _mcp_tools(monkeypatch)
    with pytest.raises(Exception, match="outside"):
        _call(mcp, "load_context", {"repo": str(tmp_path / "b"), "paths": ["f.txt"]})
    monkeypatch.setenv("BRF_AGENT_ROOT", str(tmp_path / "b"))
    mcp = _mcp_tools(monkeypatch)
    assert "B" in str(_call(mcp, "load_context", {"repo": str(tmp_path / "b"), "paths": ["f.txt"]}))


def test_symlinked_repo_resolving_outside_root_is_refused(tmp_path, monkeypatch):
    root = tmp_path / "root"
    root.mkdir()
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "x.txt").write_text("X")
    (root / "link").symlink_to(outside)
    mcp = _mcp_tools(monkeypatch, root=str(root))
    with pytest.raises(Exception, match="outside"):
        _call(mcp, "load_context", {"repo": str(root / "link"), "paths": ["x.txt"]})


def test_cli_mcp_accepts_root_option():
    from brf_agent.cli import build_parser

    args = build_parser().parse_args(["mcp", "--root", "/some/where"])
    assert args.root == "/some/where"


def test_load_file_context_refuses_filesystem_root():
    with pytest.raises(ValueError, match="filesystem root"):
        load_file_context("/", ["etc/hosts"])


@pytest.mark.parametrize(
    "name",
    [".env", ".env.local", "k.pem", "server.key", "id_rsa", "id_rsa.pub", ".git/config", "sub/.git/HEAD"],
)
def test_sensitive_files_are_denied(tmp_path, name):
    repo = tmp_path / "repo"
    target = repo / name
    target.parent.mkdir(parents=True)
    target.write_text("SECRETCONTENT")
    with pytest.raises(ValueError, match="sensitive"):
        load_file_context(repo, [name])


def test_symlink_to_sensitive_file_is_denied(tmp_path):
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / ".env").write_text("A=1")
    (repo / "innocent.txt").symlink_to(repo / ".env")
    with pytest.raises(ValueError, match="sensitive"):
        load_file_context(repo, ["innocent.txt"])


def test_agent_tools_enforce_root(tmp_path, monkeypatch):
    from brf_agent.agent import create_impact_orchestrator

    root = tmp_path / "root"
    root.mkdir()
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "hosts").write_text("TOPSECRETHOSTS")
    agent = create_impact_orchestrator(transport=BrfTransport(command=["brf"]), root=root)
    tool = next(t for t in agent.tools if t.name == "load_bounded_file_context")
    from agents.tool_context import ToolContext

    payload = json.dumps({"repo": str(outside), "paths": ["hosts"]})
    ctx = ToolContext(context=None, tool_name=tool.name, tool_call_id="1", tool_arguments=payload)
    out = asyncio.run(tool.on_invoke_tool(ctx, payload))
    assert "TOPSECRETHOSTS" not in str(out)
    assert "outside" in str(out)


# P2
def test_scrubber_redacts_all_shapes(tmp_path):
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "s.txt").write_text(SAMPLE)
    content = load_file_context(repo, ["s.txt"]).files[0].content
    for leak in LEAKS:
        assert leak not in content, leak
    assert "BEGIN RSA PRIVATE KEY" not in content
    assert content.count("[REDACTED]") >= 10
    assert "host/db" in content  # non-secret parts survive


# P3
def test_redact_uses_shared_scrubber_without_truncation():
    from brf_agent.transport import _redact

    text = "head OPENAI_API_KEY: sk-x123 mid Authorization: Bearer tok456 and GITHUB_TOKEN=ghp_second tail-kept"
    out = _redact(text)
    for leak in ("sk-x123", "tok456", "ghp_second"):
        assert leak not in out
    assert out.startswith("head ")
    assert out.endswith("tail-kept")


def test_transport_error_hides_colon_style_secrets(tmp_path):
    fake = tmp_path / "f.py"
    fake.write_text(
        "import sys\nsys.stderr.write('bad OPENAI_API_KEY: sk-colon-leak\\nAuthorization: Bearer sk-live-KEY\\nend')\nsys.exit(3)\n"
    )
    with pytest.raises(BrfTransportError) as exc:
        BrfTransport([sys.executable, str(fake)]).scan("x")
    msg = str(exc.value)
    assert "sk-colon-leak" not in msg and "sk-live-KEY" not in msg and "end" in msg


# P4
def test_non_utf8_output_is_wrapped(tmp_path):
    fake = tmp_path / "f.py"
    fake.write_text("import sys\nsys.stdout.buffer.write(b'\\xff\\xfe not json')\n")
    with pytest.raises(BrfTransportError):
        BrfTransport([sys.executable, str(fake)]).scan("x")


# P5
def test_timeout_is_wrapped(tmp_path):
    fake = tmp_path / "f.py"
    fake.write_text("import time\ntime.sleep(10)\n")
    t = BrfTransport([sys.executable, str(fake)], scan_timeout=0.3)
    with pytest.raises(BrfTransportError, match="timed out"):
        t.scan("x")


def test_empty_binary_is_clear_error():
    with pytest.raises(BrfTransportError, match="empty"):
        BrfTransport("")
    with pytest.raises(BrfTransportError, match="empty"):
        BrfTransport([])


# P6
def test_dash_values_are_not_parsed_as_flags(tmp_path):
    fake = tmp_path / "f.py"
    fake.write_text(
        "import sys, json\nprint(json.dumps(sys.argv[1:]), file=sys.stderr)\nsys.exit(9)\n"
    )
    t = BrfTransport([sys.executable, str(fake)])
    with pytest.raises(BrfTransportError) as exc:
        t.analyze("-weird-repo", "-remove the thing")
    argv = json.loads(str(exc.value).split(": ", 1)[1])
    assert "--change=-remove the thing" in argv
    assert argv[-2:] == ["--", "-weird-repo"]
    assert "--change" not in argv


# P7
def test_score_out_of_10_rule_and_monotonic_above_ten():
    assert [score_out_of_10(v) for v in (10, 11, 14, 15, 55, 100)] == [1, 1, 1, 2, 6, 10]
    assert score_out_of_10(25) == 3  # half rounds up, not to even
    assert score_out_of_10(0) == 0 and score_out_of_10(7) == 1
    above = [score_out_of_10(v) for v in range(0, 101)]
    assert above == sorted(above)


def test_missing_extra_names_the_extra(monkeypatch):
    import sys

    from brf_agent import extras

    monkeypatch.setitem(sys.modules, "agents", None)  # makes `import agents` raise ImportError
    with pytest.raises(extras.MissingExtraError, match=r"brf-agent\[agents\]"):
        extras.require("agents", "agents")
