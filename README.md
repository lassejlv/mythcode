# mini-code

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
mini-code

# One-shot prompt
mini-code "explain this repo"

# Run against a project
mini-code -p ./project "fix the tests"

# Debug mode
mini-code --debug
```

## REPL commands

`/exit` `/clear` `/cwd` `/new` `/model` `/help`

## Tips

- `/` for commands, `@` for file mentions
- `Tab` to insert suggestions
- `Ctrl+C` once to cancel, twice to exit
