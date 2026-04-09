# Mythcode

A blazing-fast Rust CLI for interacting with ACP-compatible coding agents directly from your terminal. Built for developers who want the power of AI-assisted coding without leaving their workflow.

## Features

- **Interactive TUI** — Beautiful terminal interface with syntax highlighting and real-time streaming
- **File Context** — Reference specific files and code with `@` mentions
- **Slash Commands** — Quick access to common actions with `/` commands
- **Multi-Provider** — Works with `opencode`, `codex`, `claude`, and `pi` agents
- **Project-Aware** — Scope conversations to specific directories

## Install

```bash
npm install -g @mythcode/cli
```

```bash
bun install -g @mythcode/cli
```

Or download pre-built binaries from the [latest release](https://github.com/lassejlv/minicode/releases).

**Requirements:** Rust stable + one agent provider (`opencode`, `codex`, `claude`, or `pi`).

## Quick Start

```bash
# Start the interactive TUI
mythcode

# One-shot prompt
mythcode "explain this function"

# Work within a project directory
mythcode -p ./my-app "fix the failing tests"
```

## Usage

### Interactive Mode

Launch the full TUI by running `mythcode` with no arguments. You'll get a polished interface with:
- Streaming responses from your agent
- Syntax-highlighted code blocks
- Inline file references
- Expandable tool outputs

### One-Shot Mode

Pass your prompt as an argument for quick, single-turn interactions:

```bash
mythcode "write a README for this project"
mythcode -p ./api "add error handling to the endpoints"
```

## Command Options

| Flag | Description | Default |
|------|-------------|---------|
| `-p, --project <PATH>` | Working directory for the session | Current directory |
| `--model <MODEL>` | Specific model to use | Provider default |
| `--provider <PROVIDER>` | Agent provider | Auto-detected |
| `--debug` | Enable verbose output | Disabled |

## TUI Controls

| Key | Action |
|-----|--------|
| `Enter` | Submit message |
| `Ctrl+D` | Exit the application |
| `@` | Mention files in context |
| `/` | Access slash commands |
| `Tab` / `↑` / `↓` | Navigate suggestions |
| `Ctrl+O` | Expand/collapse tool output |

## License

MIT
