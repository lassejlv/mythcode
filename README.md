# Myth Code

A Rust CLI that talks to AI-agents over ACP.

Currently only supports opencode. More to come.

## Requirements

- Rust stable
- `opencode` in `PATH`

## Install

```bash
cargo build --release
```

## Usage

```bash
# Interactive REPL
mythcode

# One-shot prompt
mythcode "explain this repo"

# Run against a project
mythcode -p ./project "fix the tests"

# Debug mode
mythcode-code --debug
```

## REPL commands

`/exit` `/clear` `/cwd` `/new` `/model` `/help`

## Tips

- `/` for commands, `@` for file mentions
- `Tab` to insert suggestions
- `Ctrl+C` once to cancel, twice to exit
