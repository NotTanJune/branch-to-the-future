# Codex Context Handoff

This file carries context from the previous Codex session because chat history does not automatically transfer when starting Codex in a copied or renamed local folder.

## Repository Link

- Primary folder: `/Users/nottanjune/Code-Projects/branch-futures`
- Temporary copied folder may exist at `/Users/nottanjune/Code-Projects/branch-to-the-future`
- Canonical repo URL: `https://github.com/NotTanJune/branch-futures`
- Package name in `Cargo.toml`: `branch-futures`
- Binary command: `brf`
- Library crate name remains `branch_futures`

## README And Branding

README was updated to use the product name `Branch Futures`.

README title includes this Tenor GIF next to the title:

```html
<img src="https://media1.tenor.com/m/cDYTvaH9nlYAAAAd/pixel-art-delorean.gif" width="56" alt="Pixel art DeLorean">
```

README now documents:

- Install and reinstall steps
- `.env` setup for OpenAI and GitHub tokens
- Local repo and GitHub repo usage
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
- Temp clone root is `branch-futures-clones`.
- GitHub token keys are supported through env or `.env`: `GITHUB_TOKEN`, `GH_TOKEN`, `GITHUB_PAT`, `GITHUB_PERSONAL_ACCESS_TOKEN`.
- Token is sent through a Git HTTP extra header, not embedded in the clone URL.

Analysis:

- OpenAI analysis uses strict JSON schema.
- Parser normalizes common model variants before typed serde parsing.
- It accepts camelCase fields, scalar strings where arrays are expected, title-case risk and complexity, numeric strings, and `0.0` to `1.0` float scores.
- Streaming truncated JSON triggers a retry with a larger output cap, up to `16000`.
- Default output token cap is `8000`.
- Model env var: `BRANCH_FUTURES_MODEL`.

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

- Markdown report filename is `branch-futures-report.md`.
- Report title is `Branch Futures Report`.
- The section `Branch Futures` is still used for implementation alternatives. This is intentional for now because it names the concept.

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
- `tests/mvp.rs`: integration tests

## Verification Already Run

In `/Users/nottanjune/Code-Projects/branch-futures`:

```bash
cargo test
cargo fmt --check
cargo install --path . --force
brf --help
```

All tests passed when run before this handoff.

## Copy Caveat

If using the temporary copied folder, make sure its `origin` remote points to `https://github.com/NotTanJune/branch-futures.git`.
