<h1>
  <img src="https://media1.tenor.com/m/cDYTvaH9nlYAAAAd/pixel-art-delorean.gif" width="56" alt="Pixel art DeLorean">
  Branch to the Future
</h1>

Branch to the Future is a terminal-native change impact simulator. You describe a planned code change, it scans a local repository or a GitHub repository link, asks OpenAI for a structured blast-radius analysis, shows the result in a Ratatui TUI, and exports it to Notion.

It is not a code generator. It helps you understand what a change may touch before you implement it.

## Demo
https://github.com/user-attachments/assets/2177c491-30cb-4789-8b9c-be0a9e9a7d19

## Requirements

- Rust toolchain
- Python 3.11+ and `uv`, required for `brf-agent`
- Git, required for GitHub repo links
- OpenAI API key
- Notion CLI `ntn`, required for TUI export
- Terminal with interactive TUI support

## Install

Install the `brf` binary locally:

```bash
make install
```

Equivalent Cargo command:

```bash
cargo install --path . --force
```

After changing source code, reinstall before using the installed `brf` binary again:

```bash
make install
```

Lean build without screen transition effects (drops the `tachyonfx` dependency tree):

```bash
cargo install --path . --force --no-default-features
```

HTTPS uses rustls with bundled root certificates, so no OpenSSL or other system library is needed on macOS, Linux or Windows.

`cargo run` and `make demo` use current source. The installed `brf` command only changes after reinstall.

## Configuration

Create a `.env` file in this project directory, in the directory where you run `brf`, or in the repository you scan:

```env
OPENAI_API_KEY=sk-your-key-here
```

For private GitHub repositories, add one of these token keys:

```env
GITHUB_TOKEN=ghp-your-token-here
GH_TOKEN=ghp-your-token-here
GITHUB_PAT=ghp-your-token-here
GITHUB_PERSONAL_ACCESS_TOKEN=ghp-your-token-here
```

Shell environment wins over `.env`:

```bash
export OPENAI_API_KEY=sk-your-key-here
export GITHUB_TOKEN=ghp-your-token-here
```

Optional model override:

```bash
export BRANCH_TO_THE_FUTURE_MODEL=gpt-5.2
```

GitHub authentication uses `git clone --depth 1` with an in-memory HTTP extra header. The token is not written into the clone URL.

For TUI export, install the Notion CLI and set a Notion token. Branch to the Future calls `ntn api` for page creation and updates, forwards this token to `ntn` as `NOTION_API_TOKEN`, and supplies `NOTION_API_VERSION=2026-03-11` unless you already set a version:

```bash
curl -fsSL https://ntn.dev | bash
export NOTION_TOKEN=ntn_your_token_here
```

`NOTION_API_KEY` is also accepted. The token needs `insert_content` and `update_content` capabilities. Share the target parent page with the integration, unless you use a personal access token that already has page access.

If you do not set a token, authenticate the Notion CLI:

```bash
ntn login
```

## Quick Start

Run the bundled demo:

```bash
make demo
```

Or use the installed binary:

```bash
brf sample-repos/resume-interview
```

At the prompt, enter a change request:

```text
add async resume parsing with S3 upload and status polling
```

Branch to the Future will:

1. Scan the repository.
2. Stream OpenAI analysis progress.
3. Show impacted files, sorted high to low by default.
4. Compare implementation futures.
5. Show a navigable repo tree.
6. Show a terminal-native architecture map.
7. Export to a Notion page.

## Analyze A Local Repo

```bash
brf /path/to/your/repo
```

From this source tree without installing:

```bash
make run REPO=/path/to/your/repo
```

## Analyze A GitHub Repo

```bash
brf https://github.com/owner/repo
```

Example:

```bash
brf https://github.com/NotTanJune/locator
```

GitHub links are cloned into a temporary directory under your system temp folder:

```text
<temp>/branch-to-the-future-clones/<owner>-<repo>-<uuid>
```

The scan, repo tree, architecture map, and analysis all read from that temp clone. Default report output also points at the clone, so use `--output-dir` for permanent reports:

```bash
brf https://github.com/owner/repo --output-dir ./output
```

## CLI Options

```text
brf [OPTIONS] <REPO_PATH_OR_GITHUB_URL>

Options:
  --max-file-bytes <n>          Skip files larger than n bytes. Default: 200000
  --ignore <pattern>            Repeatable ignore pattern
  --output-dir <path>           Report output directory. Default: target repo root
  --text-model <model>          OpenAI text model. Default: gpt-5-mini
  --reasoning-effort <level>    none, low, medium, high, xhigh. Default: low
  --max-output-tokens <n>       Response cap. Default: 8000
  --max-prompt-files <n>        Files included in prompt. Default: 80
```

Machine JSON modes:

```bash
brf scan --json /path/to/repo
brf analyze --json /path/to/repo --change "add async resume parsing"
```

`scan --json` does not require `OPENAI_API_KEY`. `analyze --json` scans the repo, runs the same structured OpenAI impact analysis as the TUI, and emits a versioned JSON envelope:

```json
{
  "schema_version": 1,
  "tool": "branch-to-the-future",
  "created_at": "2026-05-10T00:00:00Z",
  "change_request": "add async resume parsing",
  "repo_model": {},
  "impact_analysis": {},
  "limits": {},
  "model": {},
  "warnings": []
}
```

Fixture envelopes live in `fixtures/schema/` and are parsed by both Rust and Python tests.

## Watch Mode And Blast Radius

`brf blast` reports which files a working-tree diff can break. It is deterministic, needs no API key, and only works on local repos.

```bash
brf blast --json .                         # git diff vs HEAD plus untracked files
brf blast --json . --path src/repo.rs      # also treat a file as a proposed change
brf watch .                                # live TUI, rescans when the diff changes
```

Flags: `--path <P>` (repeatable; absolute paths must sit inside the repo), `--max-file-bytes`, `--ignore`. `watch` also takes `--interval-ms` (default 1000) and the same model flags as `analyze`.

JSON fields: `schema_version`, `tool`, `kind` (`"blast"`), `created_at`, `repo_root`, `changed` (`path`, `status`: modified, added, deleted, renamed, untracked, proposed), `impacted` (`path`, `depth` 1 or 2, `via`, `is_test`), `tests`, `untested_changes`, `routes`, `risk_signals`, `sensitive`, `score` (0 to 10), `level` (low, medium, high), `reasons`.

Imports are resolved by heuristic (JS/TS relative and `@/` or `~/` aliases, Python dotted modules, Rust `use` paths). Depth 1 files import a changed file; depth 2 files import a depth 1 file.

Watch keys: `q` or `Esc` quit, `Tab`/`Shift-Tab` (or `h`/`l`) switch between the Changed, Blast radius and Summary panels (the selected one has a cyan border), `j`/`k` move the highlighted row, `g`/`G` jump to the ends, `PageUp`/`PageDown` move ten rows, `r` rescan (the header shows `rescanned` or `rescanned, no change`), `a` run OpenAI analysis of the diff (needs `OPENAI_API_KEY`; a new diff marks the result stale).

## Python Agent Sidecar

The core package only needs Pydantic. The OpenAI Agents SDK and the MCP server are optional extras, so install only what you use:

| Install | Adds | Needed for |
|---|---|---|
| `brf-agent` | Pydantic | `analyze` and the Python API (transport, schemas, reports) |
| `brf-agent[agents]` | `openai-agents` | `chat` and the Impact Orchestrator |
| `brf-agent[mcp]` | `mcp` | `brf-agent mcp` |
| `brf-agent[all]` | both | everything |

```bash
uv pip install .            # core: 6 packages, about 7 MB
uv pip install '.[all]'     # agents + MCP: 39 packages, about 54 MB
```

A command whose extra is missing exits with the exact install command. Inside this repo, `uv run` installs the dev group, which includes every extra.

Run with `uv`:

```bash
uv run brf-agent analyze /path/to/repo --change "add async resume parsing"
uv run brf-agent analyze /path/to/repo --change "add async resume parsing" --json
uv run brf-agent chat session-1
uv run brf-agent mcp
```

`brf-agent analyze` calls the installed `brf` binary through subprocess transport, parses the versioned envelope with Pydantic, and renders a human report with impact, risk, architecture, evidence, test, patch, and JSON appendix sections.

The Python package includes:

- Pydantic schema mirrors for the Rust serde envelope.
- Subprocess transport for `brf scan --json` and `brf analyze --json`.
- Bounded read-only file context loading with outside-root path rejection and basic secret redaction.
- Patch plan and draft unified diff generation. Drafts are text only; V1 has no file mutation API.
- OpenAI Agents SDK `Impact Orchestrator` with function tools and structured output.
- SDK session-backed `brf-agent chat` using `SQLiteSession`.
- MCP stdio server exposing scan, analyze, context, report, and patch-plan tools.

`brf-agent mcp` pins an allowed root at startup (`--root`, else `BRF_AGENT_ROOT`, else the current directory). Every tool's `repo` must resolve inside it, the filesystem root is refused, and `.git/`, `.env*`, `*.pem`, `*.key` and `id_rsa*` are never read. Secrets in file context and error messages are redacted.

Useful local run:

```bash
brf /path/to/your/repo \
  --max-file-bytes 200000 \
  --ignore vendor \
  --ignore tmp \
  --output-dir /path/to/reports \
  --text-model gpt-5-mini \
  --reasoning-effort low \
  --max-output-tokens 8000 \
  --max-prompt-files 80
```

Deeper run:

```bash
brf /path/to/your/repo \
  --text-model gpt-5.2 \
  --reasoning-effort high \
  --max-output-tokens 16000 \
  --max-prompt-files 200
```

## TUI Controls

Global controls:

```text
q             Quit
?             Help
Esc           Back where active
e             Export to Notion after analysis
g             Open architecture after analysis
T             Open repo tree after scan
```

Impact Explorer:

```text
j/k or arrows Select impact file
Enter         Inspect selected file
s             Cycle impact sort: high to low, low to high, model order
Tab           Open futures
g             Open architecture
T             Open repo tree
e             Export to Notion
```

File Detail:

```text
j/k or arrows Next or previous impacted file
s             Cycle impact sort
Esc           Back to impact explorer
g             Open architecture
T             Open repo tree
```

Futures:

```text
j/k or arrows Select future
Tab or Esc    Back to impact explorer
g             Open architecture
T             Open repo tree
e             Export to Notion
```

Repo Tree:

```text
j/k or arrows Select repo file
Esc           Return to previous screen
g             Open architecture
e             Export to Notion
```

Architecture:

```text
h/l or arrows Pan left and right
j/k or arrows Pan up and down
PageUp        Pan up faster
PageDown      Pan down faster
-             Zoom out to compact map
+ or =        Zoom in to normal map
0             Reset pan and zoom
Esc           Return to previous screen
T             Open repo tree
e             Export to Notion
```

Ratatui does not control terminal font zoom. Branch to the Future uses `Paragraph::scroll((row, col))` for panning and an app-level compact map mode for zoom-out behavior. Compact mode narrows architecture boxes and wraps labels inside them without truncating text.

## What The Screens Show

Input:

- Change request prompt.

Repo Scan:

- Repository materialization progress.
- OpenAI stream progress.
- Parsed repo counts: files, routes, tests, frameworks.

Impact Explorer:

- Impact path.
- Impact scores normalized to `/10`.
- Confidence and risk.
- Sort state.

File Detail:

- Selected file reason, risk, confidence, change needed.

Futures:

- Alternative implementation paths.
- Complexity, risk, benefits, drawbacks, patch plan, test plan.

Repo Tree:

- Traversable full-screen repository file list.
- Selected file details: kind, size, symbols, imports, snippets.

Architecture:

- Current architecture from model-generated stages.
- Proposed architecture from selected future.
- Box and arrow system map.
- Change set and risk signals.
- Scroll and compact zoom for large maps.

Export:

- Notion page URL and create or update status.

## App Architecture

```text
+-------------------------+
| CLI and startup         |
| src/main.rs             |
| src/cli.rs              |
+-----------+-------------+
            |
            v
+-------------------------+       +-------------------------+
| Repo source resolver    | ----> | Temporary GitHub clone  |
| src/repo_source.rs      |       | git clone --depth 1     |
| .env GitHub token load  |       | temp/branch-to-the-future-clones |
+-----------+-------------+       +-----------+-------------+
            |                                 |
            +---------------+-----------------+
                            |
                            v
+-------------------------+       +-------------------------+
| Repository scanner      | ----> | RepoModel               |
| src/repo.rs             |       | files, routes, tests    |
| file kinds and snippets |       | frameworks, risk hints  |
+-----------+-------------+       +-----------+-------------+
                            |
                            v
+-------------------------+       +-------------------------+
| OpenAI analysis client  | ----> | ImpactAnalysis          |
| src/ai.rs               |       | impact path, futures    |
| streaming JSON schema   |       | architecture stages     |
| retry and normalization |       | normalized scores       |
+-----------+-------------+       +-----------+-------------+
                            |
                            v
+-----------------------------------------------------------+
| App state and key handling                                |
| src/app.rs                                                |
| screens, selection, sorting, repo tree return context,    |
| architecture pan, architecture zoom, export state         |
+-----------------------------+-----------------------------+
                              |
                              v
+-------------------------+       +-------------------------+
| Ratatui rendering       | ----> | TachyonFX transitions   |
| src/ui.rs               |       | src/fx.rs               |
| explorer, tree, map     |       | screen effects          |
+-----------+-------------+       +-----------+-------------+
            |
            v
+-------------------------+
| Notion export           |
| src/notion.rs           |
| ntn api over stdin JSON |
+-------------------------+
```

Blast radius and watch mode run beside the main TUI and need no API key:

```text
+-------------------------+       +-------------------------+
| brf watch               | ----> | brf blast --json        |
| src/watch.rs            |       | src/blast.rs            |
| panels, cursor, AI key  |       | git diff + import graph |
|                         | <---- | score, level, reasons   |
+-------------------------+       +-------------------------+
```

## Analysis Structure

OpenAI is asked for strict JSON using a schema. Branch to the Future then normalizes common model shape variants before typed parsing:

- camelCase fields such as `impactPath` and `recommendedFuture`
- string values where arrays are expected
- title-case risk and complexity values such as `High`
- numeric strings and `0.0` to `1.0` float scores
- model-generated architecture stages for any language or framework

If streamed JSON is truncated, Branch to the Future retries automatically with a larger output cap, up to `16000` tokens.

## What Gets Scanned

Branch to the Future performs shallow multi-language scanning:

- JavaScript and TypeScript files
- Python files
- Rust files
- Config files such as `package.json`, `tsconfig.json`, `pyproject.toml`, `Cargo.toml`
- Route-like files
- Test files, including Rust integration tests under `tests/*.rs`
- Schema and database files
- Worker, service, controller, and component paths

It extracts imports, symbol-like names, short snippets, routes, framework hints, tests, config files, and simple risk signals.

Binary-like assets are skipped. Files over `--max-file-bytes` are skipped. Git ignore rules and repeated `--ignore` patterns are respected.

## Notion Export

Press `e` after analysis to open the Notion export prompt.

The app always uses `ntn api` for page creation and updates. Preferred auth is `NOTION_TOKEN` or `NOTION_API_KEY`; Branch to the Future forwards either value to `ntn` as `NOTION_API_TOKEN`. If neither is set, `ntn` uses its own login state from `ntn login`.

On first export, enter a parent page, database, or data-source ref:

```text
page:<id>
database:<id>
data-source:<id>
```

You can also paste a Notion page URL or raw page ID. The app creates a Notion page with Notion-optimized Markdown.

After a page is created in the same TUI session, press `e` again and hit Enter on an empty prompt to update that same page. Type a new parent ref or URL to create a new page instead.

The Notion page includes:

- Change request
- Repo summary
- Impact path
- Risk summary
- Current and proposed architecture stages
- Evidence per impacted file
- Branch futures
- Recommended path
- Test plan
- Patch plan

If `ntn login` fails with workspace governance errors, use `NOTION_TOKEN` or `NOTION_API_KEY` instead. Page creation and updates still run through `ntn api`.

## Troubleshooting

GitHub says `terminal prompts disabled`:

- Add `GITHUB_TOKEN`, `GH_TOKEN`, or `GITHUB_PAT` to `.env`.
- Or export one in your shell.
- Fine-grained tokens must have access to the target repo.

`invalid impact analysis JSON syntax`:

- The app retries automatically with more output tokens.
- If it still fails, rerun with `--max-output-tokens 16000`.
- For very large repos, also lower prompt size with `--max-prompt-files 80` or scan a narrower repo path.

Installed `brf` does not reflect source changes:

```bash
make install
```

## Development

Run tests:

```bash
make test
uv run pytest
```

Format code:

```bash
make fmt
```

Build:

```bash
make build
```

Run current source without installing:

```bash
cargo run -- sample-repos/resume-interview
```
