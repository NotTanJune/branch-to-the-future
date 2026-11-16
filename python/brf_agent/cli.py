from __future__ import annotations

import argparse
import asyncio
import json
import sys

from .agent import run_agentic_turn
from .mcp_server import run_mcp
from .patch import draft_patch_plan
from .reports import render_markdown_report
from .extras import MissingExtraError
from .transport import BrfTransport, BrfTransportError


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        if args.command == "analyze":
            return analyze(args)
        if args.command == "chat":
            return asyncio.run(chat(args.session_id))
        if args.command == "mcp":
            run_mcp(args.root)
            return 0
    except (BrfTransportError, MissingExtraError, ValueError) as exc:
        print(f"brf-agent: {exc}", file=sys.stderr)
        return 1
    return 2


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="brf-agent")
    subparsers = parser.add_subparsers(dest="command", required=True)

    analyze_parser = subparsers.add_parser("analyze")
    analyze_parser.add_argument("repo")
    analyze_parser.add_argument("--change", required=True)
    analyze_parser.add_argument("--json", action="store_true")
    analyze_parser.add_argument("--brf-binary")

    chat_parser = subparsers.add_parser("chat")
    chat_parser.add_argument("session_id")

    mcp_parser = subparsers.add_parser("mcp")
    mcp_parser.add_argument(
        "--root",
        help="allowed repo root for every tool (default: BRF_AGENT_ROOT or the current directory)",
    )
    return parser


def analyze(args: argparse.Namespace) -> int:
    transport = BrfTransport(command=args.brf_binary)
    envelope = transport.analyze(args.repo, args.change)
    if args.json:
        print(envelope.model_dump_json(indent=2))
    else:
        print(render_markdown_report(envelope))
        plan = draft_patch_plan(envelope)
        if plan.files:
            print("\nExport paths:")
            for path in plan.files:
                print(f"- {path}")
    return 0


async def chat(session_id: str) -> int:
    print("brf-agent chat. Press Ctrl-D to exit.")
    while True:
        try:
            prompt = input("> ")
        except EOFError:
            print()
            return 0
        if not prompt.strip():
            continue
        output = await run_agentic_turn(prompt, session_id=session_id)
        if hasattr(output, "model_dump"):
            print(json.dumps(output.model_dump(mode="json"), indent=2))
        else:
            print(output)
