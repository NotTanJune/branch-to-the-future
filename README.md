# Branch Futures

Branch Futures is a terminal-native change impact simulator. You describe a planned code change, it scans a local repository, asks OpenAI for a blast-radius analysis, shows the result in a Ratatui TUI, and exports a Markdown report.

It is not a code generator. It helps you understand what a change may touch before you implement it.

## Requirements

- Rust toolchain
- OpenAI API key
- Terminal with interactive TUI support

## Setup

Create a `.env` file in this project directory or in the repository you want to scan:

```env
OPENAI_API_KEY=sk-your-key-here
```

Process environment wins over `.env` if both are set:

```bash
export OPENAI_API_KEY=sk-your-key-here
```

Optional model override:

```bash
export BRANCH_FUTURES_MODEL=gpt-5.2
```

## Install

Install the binary locally:

```bash
make install
```

Equivalent Cargo command:

```bash
cargo install --path . --force
```

After install, run the app directly:

```bash
brf sample-repos/resume-interview
```

If you change source code after installing, reinstall before using `brf` again:

```bash
make install
```

`cargo run` and `make demo` always use current source. The installed `brf` binary only changes after reinstall.

## Simple Commands

From this project directory:

```bash
make demo
```

Run against your own repo:

```bash
make run REPO=/path/to/your/repo
```

Show available shortcuts:

```bash
make help
```

## Run The Demo

Use the bundled sample repository:

```bash
make demo
```

Or use the installed binary:

```bash
brf sample-repos/resume-interview
```

At the prompt, enter:

```text
add async resume parsing with S3 upload and status polling
```

Branch Futures will:

1. Scan the sample repo.
2. Stream the repo summary and change request response from OpenAI.
3. Show likely impacted files.
4. Compare implementation futures.
5. Let you generate and view an architecture diagram in the TUI.
6. Let you export a Markdown report at the target repo root.

## Scan Your Own Repo

```bash
brf /path/to/your/repo
```

From the source tree without installing:

```bash
make run REPO=/path/to/your/repo
```

Useful options:

```bash
brf /path/to/your/repo \
  --max-file-bytes 200000 \
  --ignore vendor \
  --ignore tmp \
  --output-dir /path/to/your/repo \
  --text-model gpt-5-mini \
  --reasoning-effort low \
  --max-output-tokens 2500 \
  --max-prompt-files 80
```

For a deeper run:

```bash
brf /path/to/your/repo \
  --text-model gpt-5.2 \
  --reasoning-effort high \
  --max-output-tokens 6000 \
  --max-prompt-files 200
```

CLI shape:

```text
brf [OPTIONS] <REPO_PATH>

Options:
  --max-file-bytes <n>          Skip files larger than n bytes. Default: 200000
  --ignore <pattern>            Repeatable ignore pattern
  --output-dir <path>           Report output directory. Default: target repo root
  --text-model <model>          OpenAI text model. Default: gpt-5-mini
  --reasoning-effort <level>    none, low, medium, high, xhigh. Default: low
  --max-output-tokens <n>       Response cap. Default: 2500
  --max-prompt-files <n>        Files included in prompt. Default: 80
```

## Cost Controls

Default settings are optimized for prototyping:

- `gpt-5-mini`
- `reasoning-effort low`
- `max-output-tokens 2500`
- `max-prompt-files 80`
- compact repo prompt with truncated symbols, imports, and snippets

Use `gpt-5.2` and higher limits only when the prototype result is too shallow.

## TUI Controls

```text
q           Quit
Esc         Back
Tab         Switch explorer and futures
Shift+Tab   Switch explorer and futures
Enter       Inspect selected item
j / Down    Move down
k / Up      Move up
r           Replay impact trace
p           Show patch skeleton status
t           Show test plan status
g           Generate architecture diagram
e           Export Markdown report
?           Help
```

The input screen shows a blinking cursor. During analysis, the scan screen shows repository progress and OpenAI stream deltas while the final structured JSON is still being generated.

Press `g` after analysis to generate a high-quality architecture blueprint with OpenAI image generation. The image call uses `gpt-5.2` with the hosted image generation tool, opens a terminal diagram view, and writes the PNG to the target repo root.

The saved PNG is decorative and exportable: it provides the blueprint background, zones, boxes, and arrows. The readable file names, scores, risks, selected trace path, branch futures, recommendation, and saved path are rendered by the TUI as real terminal text over the preview. This keeps labels legible after terminal downsampling.

## Output

Press `e` after analysis to export the Markdown report at the target repo root:

```text
<repo-path>/branch-futures-report.md
```

Press `g` after analysis to generate:

```text
<repo-path>/branch-futures-architecture.png
```

The diagram is displayed inside the TUI first. The saved PNG remains at the repo root so you can use it in the report or inspect it outside the terminal.

After changing source code, run `make install` or `cargo install --path . --force` before using the installed `brf` binary. `cargo run` uses current source without reinstalling.

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
- Image artifact path, when generated

## What Gets Scanned

Branch Futures performs shallow multi-language scanning:

- JavaScript and TypeScript files
- Python files
- Rust files
- Config files such as `package.json`, `tsconfig.json`, `pyproject.toml`, `Cargo.toml`
- Route-like files
- Test files
- Schema and database files
- Worker, service, controller, and component paths

It extracts imports, symbol-like names, short snippets, routes, framework hints, tests, config files, and simple risk signals.

## Current Limits

- Requires real OpenAI access. No runtime mock fallback.
- Does not apply patches.
- Architecture diagram generation requires the OpenAI image generation tool.
- Scanner is shallow by design. It is a prompt context builder, not a full compiler or static analyzer.
- OpenAI failures show a retryable error screen instead of fake analysis.

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
