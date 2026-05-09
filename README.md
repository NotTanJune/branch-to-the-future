# Branch Futures

Branch Futures is a terminal-native change impact simulator. You describe a planned code change, it scans a local repository or a GitHub repository link, asks OpenAI for a structured blast-radius analysis, shows the result in a Ratatui TUI, and exports a Markdown report.

It is not a code generator. It helps you understand what a change may touch before you implement it.

## Requirements

- Rust toolchain
- Git, required for GitHub repo links
- OpenAI API key
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
export BRANCH_FUTURES_MODEL=gpt-5.2
```

GitHub authentication uses `git clone --depth 1` with an in-memory HTTP extra header. The token is not written into the clone URL.

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

Branch Futures will:

1. Scan the repository.
2. Stream OpenAI analysis progress.
3. Show impacted files, sorted high to low by default.
4. Compare implementation futures.
5. Show a navigable repo tree.
6. Show a terminal-native architecture map.
7. Export a Markdown report.

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
<temp>/branch-futures-clones/<owner>-<repo>-<uuid>
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
e             Export Markdown report after analysis
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
e             Export Markdown
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
e             Export Markdown
```

Repo Tree:

```text
j/k or arrows Select repo file
Esc           Return to previous screen
g             Open architecture
e             Export Markdown
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
e             Export Markdown
```

Ratatui does not control terminal font zoom. Branch Futures uses `Paragraph::scroll((row, col))` for panning and an app-level compact map mode for zoom-out behavior. Compact mode narrows architecture boxes and wraps labels inside them without truncating text.

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

- Markdown report path.

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
| .env GitHub token load  |       | temp/branch-futures... |
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
| Markdown export         |
| src/artifacts.rs        |
| branch-futures-report.md|
+-------------------------+
```

## Analysis Structure

OpenAI is asked for strict JSON using a schema. Branch Futures then normalizes common model shape variants before typed parsing:

- camelCase fields such as `impactPath` and `recommendedFuture`
- string values where arrays are expected
- title-case risk and complexity values such as `High`
- numeric strings and `0.0` to `1.0` float scores
- model-generated architecture stages for any language or framework

If streamed JSON is truncated, Branch Futures retries automatically with a larger output cap, up to `16000` tokens.

## What Gets Scanned

Branch Futures performs shallow multi-language scanning:

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

## Output

Press `e` after analysis to export:

```text
<output-dir>/branch-futures-report.md
```

The report includes:

- Change request
- Repo summary
- Impact path
- Affected files
- Risk summary
- Branch futures
- Recommended path
- Test plan
- Patch skeleton
- Architecture scaffold note

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
