# Agent Instructions

## Communication

Use caveman lite style: concise, professional, no filler. Keep technical terms exact. Do not use em dashes in reports or documents.

## Session Bootstrap

Before making changes, read `CODEX_CONTEXT.md`. It is the handoff file for recent implementation details, verification status, and open caveats.

## Project Status

The repo link expected by the user is `https://github.com/NotTanJune/branch-futures`. The binary command remains `brf`. The Rust library crate remains `branch_futures`.

## Workflow

- Prefer existing project patterns.
- Use `rg` for text search and `cymbal` for code navigation when available.
- Run `cargo test` and `cargo fmt --check` before claiming completion.
- After source changes that affect the CLI, run `cargo install --path . --force` so the installed `brf` command matches source.
- Do not push, deploy, or submit external jobs without explicit user confirmation.
