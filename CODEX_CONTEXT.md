# Codex Context Handoff

This file carries context from the previous Codex session because chat history does not automatically transfer when starting Codex in a copied or renamed local folder.

## Repository Link

- Primary folder: `/Users/nottanjune/Code-Projects/branch-to-the-future`
- Canonical repo URL: `https://github.com/NotTanJune/branch-to-the-future`
- Package name in `Cargo.toml`: `branch-to-the-future`
- Binary command: `brf`
- Library crate name remains `branch_futures`

## README And Branding

README was updated to use the product name `Branch to the Future`.

README title includes this Tenor GIF next to the title:

```html
<img src="https://media1.tenor.com/m/cDYTvaH9nlYAAAAd/pixel-art-delorean.gif" width="56" alt="Pixel art DeLorean">
```

README now documents:

- Install and reinstall steps
- `.env` setup for OpenAI and GitHub tokens
- Local repo and GitHub repo usage
- Machine JSON modes: `brf scan --json` and `brf analyze --json`
- Python sidecar commands: `brf-agent analyze`, `brf-agent chat`, and `brf-agent mcp`
- CLI defaults, including `--max-output-tokens 8000`
- Full TUI keybindings
- Repo tree
- Architecture scroll and compact zoom
- JSON normalization and retry behavior
- ASCII app architecture wireframe

## Implemented Functionality

GitHub repo links:

- `brf https://github.com/owner/repo` is supported.
- Repo links are cloned with `git clone --depth 1`.
- Temp clone root is `branch-to-the-future-clones`.
- GitHub token keys are supported through env or `.env`: `GITHUB_TOKEN`, `GH_TOKEN`, `GITHUB_PAT`, `GITHUB_PERSONAL_ACCESS_TOKEN`.
- Token is sent through a Git HTTP extra header, not embedded in the clone URL.

Analysis:

- Rust machine output uses a versioned JSON envelope with `schema_version: 1`.
- `brf scan --json <repo>` returns scan JSON and does not require `OPENAI_API_KEY`.
- `brf analyze --json <repo> --change "<request>"` returns scan plus impact analysis JSON.
- OpenAI analysis uses strict JSON schema.
- Parser normalizes common model variants before typed serde parsing.
- It accepts camelCase fields, scalar strings where arrays are expected, title-case risk and complexity, numeric strings, and `0.0` to `1.0` float scores.
- Streaming truncated JSON triggers a retry with a larger output cap, up to `16000`.
- Default output token cap is `8000`.
- Model env var: `BRANCH_TO_THE_FUTURE_MODEL`.

Repo scanning:

- Rust integration tests under `tests/*.rs` now count as tests.
- For `https://github.com/NotTanJune/locator`, expected scan summary is `36 files, 0 routes, 6 tests, 1 frameworks`.

TUI:

- `T` opens repo tree after scan.
- `g` opens architecture after analysis.
- `s` cycles impact sort.
- Impact paths default to high-to-low.
- Architecture page uses Ratatui `Paragraph::scroll((row, col))` for pan.
- Architecture page has compact zoom mode because Ratatui cannot control terminal font zoom.
- Architecture keys: `h/j/k/l`, arrows, `PageUp`, `PageDown`, `-`, `+`, `=`, `0`.

Architecture:

- Current and proposed architecture labels are model-generated when present.
- Static boxes remain only as fallback for older or missing analysis.
- Box and arrow visualization was restored.
- Long labels are not truncated. Compact zoom wraps text inside narrower boxes.

Exports:

- Markdown report filename is `branch-to-the-future-report.md`.
- Report title is `Branch to the Future Report`.
- Markdown report now includes scan summary, impact table, current and proposed architecture stages, evidence, patch plan, and JSON appendix.
- Score rendering preserves ten-point model scores, so `7` renders as `7/10`.
- The section `Branch to the Future` is still used for implementation alternatives. This is intentional for now because it names the concept.

Python sidecar:

- `pyproject.toml` defines package `brf-agent`; use `uv` for Python commands.
- `python/brf_agent/schemas.py` mirrors Rust serde envelope fixtures in `fixtures/schema/`.
- `python/brf_agent/transport.py` calls `brf` through subprocess transport.
- `python/brf_agent/context.py` provides bounded read-only file context with outside-root rejection and basic secret redaction.
- `python/brf_agent/agent.py` defines the OpenAI Agents SDK `Impact Orchestrator`; SDK imports are lazy.
- `python/brf_agent/mcp_server.py` exposes MCP stdio tools for scan, analyze, context, report, and patch planning.
- V1 does not mutate files; patch output is plan text or draft unified diff text only.

## Important Files

- `README.md`: user-facing docs and wireframe
- `Cargo.toml`: package metadata
- `src/main.rs`: CLI startup
- `src/cli.rs`: CLI args, env model resolution
- `src/repo_source.rs`: GitHub URL parsing, token loading, temp clone
- `src/repo.rs`: repo scanner and file classification
- `src/ai.rs`: OpenAI request, streaming, JSON normalization, retry
- `src/app.rs`: app state, key handling, pan and zoom state
- `src/ui.rs`: Ratatui rendering, repo tree, architecture map
- `src/artifacts.rs`: Markdown export
- `src/machine.rs`: versioned JSON envelopes and machine CLI runners
- `python/brf_agent/`: Python CLI, SDK orchestrator, MCP server, schemas, transport, reports, context, patch draft helpers
- `fixtures/schema/`: shared Rust and Python JSON fixtures
- `tests/mvp.rs`: integration tests
- `tests/python/test_agent_sidecar.py`: Python sidecar tests

## Blast Radius And Watch

- `src/blast.rs`: `BlastReport` (`schema_version`, `tool`, `kind`, `created_at`, `repo_root`, `changed`, `impacted`, `tests`, `untested_changes`, `routes`, `risk_signals`, `sensitive`, `score`, `level`, `reasons`), `git_changed_files`, `diff_fingerprint`, `git_diff_text`, `compute_blast`, `ImportResolver`, `run_blast_json`.
- `brf blast --json <repo> [--path P]...` is deterministic and needs no API key. GitHub URLs are rejected.
- Import resolution is heuristic: JS/TS relative and `@/` `~/` aliases, Python dotted modules (with `src/` and suffix fallback), Rust `crate::`, `self::`, `super::` and crate-name paths.
- `src/watch.rs`: `brf watch <repo>` Ratatui TUI. Polls `diff_fingerprint` every `--interval-ms`, rescans on change. `a` runs OpenAI analysis in a tokio task; missing key shows "OPENAI_API_KEY not set". Terminal restored via `ratatui::init`/`restore`.
- Tests: `tests/blast.rs` (real `git init` repos), CLI parse tests in `tests/mvp.rs`, unit tests in `src/blast.rs` and `src/watch.rs`. `brf watch` was not run interactively (no TTY).

## Verification Already Run

Previous handoff verification in `/Users/nottanjune/Code-Projects/branch-to-the-future`:

```bash
cargo test
cargo fmt --check
cargo install --path . --force
brf --help
```

All tests passed when run before this handoff.

Current branch verification should include:

```bash
cargo test
cargo fmt --check
uv run pytest
brf scan --json sample-repos/resume-interview
cargo install --path . --force
brf --help
```

## Claude Code Mod

- `integrations/claude-code/brf-blast/` is a Claude Code mod (function hooks module `hooks/register.ts`, state contract `types/index.d.ts`, tests `hooks/register.test.ts`).
- It hooks `tool.call` for Edit, Write and NotebookEdit, runs `brf blast --json <repo> --path <file>` before the permission prompt, sets the status line, and on `level == "high"` raises a toast and appends a model note to the tool result. It never denies and fails open.
- Inside Rex (`REX_SESSION`, `REX_BLOCK`), it opens one right split labelled `brf` running `brf watch <repo>`; the block id is kept in `$.state` key `brf-blast.pane`.
- `/brf`, `/brf status`, `/brf close`. Verify with `claude plugin validate` and `claude plugin test` on the folder.
- Score covers the whole working tree plus the proposed file, not the single edit in isolation.
- The repo root is resolved per edited file with `git -C <dir> rev-parse --show-toplevel --show-prefix` (cached per directory on success only). The pane opens only after a successful blast; an old `brf` without `blast` is reported by `/brf status`.
- Loaded globally through `CLAUDE_CODE_PLUGIN_DIRS` in the `env` block of `~/.claude/settings.json`, pointing at this folder.

## Hardening Pass (2026-11-16)

- Scanner: default ignores match whole path segments (`builder.ts`, `build.rs`, `.github/` are scanned); `--ignore` uses gitignore glob semantics; imports are uncapped and cover Rust `mod`/brace groups, JS re-exports and dynamic imports, Python relative and comma imports.
- Blast: subdirectory roots use `git diff --relative`; git output is parsed with `-z`; staged renames report the old path as deleted; sensitive paths match on word boundaries; non-git dirs fail with `not a git repository`.
- TUI: `q` and `?` are text on the Input and Notion prompt screens (Ctrl-C quits there); Help cannot trap itself; multibyte truncation and stream preview slicing are char-safe.
- AI: SSE is buffered as bytes and split on LF or CRLF. Scores are stored 0-100: JSON integers are taken as-is, floats in 0.0..=1.0 are scaled x100, and an analysis whose scores are all 1..=10 is rescaled x10 at parse. Rendering is always `round(score / 10)` in Rust and Python.
- Notion: `ntn` stdin is written on a thread, so an early `ntn` failure reports its stderr. `.env` values drop inline comments and quotes; unreadable `.env` files are skipped.
- Python: `brf-agent mcp --root` (or `BRF_AGENT_ROOT`, else cwd) pins the allowed root; sensitive files are never read; `redact.scrub_secrets` is shared by context and transport; transport has timeouts and passes `--` before the repo.

## Dependencies

- Rust HTTP is `ureq` 3 with rustls (no OpenSSL), run on `spawn_blocking`; `reqwest`, `futures-util` and `uuid` are gone. `tachyonfx` is behind the default `fx` feature (`--no-default-features` builds without transition effects). `ratatui` uses only the `crossterm` feature.
- Release binary went from 9,510,592 to 8,430,256 bytes; normal crates from 178 to 123 on macOS (215 to 145 across all targets).
- Python core `brf-agent` depends only on Pydantic. Extras: `[agents]` (openai-agents), `[mcp]` (mcp), `[all]`. The dev group installs every extra. Missing extras raise `MissingExtraError` with the install command.
