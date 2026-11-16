# brf-blast

A Claude Code mod that runs Branch to the Future (`brf`) beside the agent.

- Every Edit, Write or NotebookEdit inside a git repo runs `brf blast --json <repo> --path <file>` before the permission prompt. The status line shows the level, score and impacted file count.
- A high radius (score 7 or more) raises a toast and adds a short note to the tool result, so the model checks impacted files and related tests. The mod never blocks an edit, and it lets the edit through if `brf` fails.
- The repo is found per edited file with `git -C <dir> rev-parse`, so a session can move between repos. Relative and `..` paths are normalized. If the installed `brf` has no `blast` subcommand, edits pass unchecked, no pane opens, and `/brf status` says so.
- Inside Rex, the first successful check opens a split on the right that runs `brf watch <repo>`. The watch TUI reruns the scan whenever the working tree changes. Press `a` there for an OpenAI analysis of the current diff.

## Commands

- `/brf` opens the watch pane, or prints the `brf watch` command when you are not in Rex.
- `/brf status` prints the blast radius of the current working tree.
- `/brf close` closes the pane.

## Requirements

- `brf` on `PATH` or at `~/.cargo/bin/brf` (`cargo install --path .` in the branch-to-the-future repo).
- Rex for the side pane. Outside Rex the status line, toast and model note still work.
- `OPENAI_API_KEY` only for the `a` key in `brf watch`.

## Develop

```bash
claude plugin validate .
claude plugin test .
claude --plugin-dir .
```
