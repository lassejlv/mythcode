# Mythcode

> A lightweight Rust CLI for interacting with AI coding agents via ACP (Agent Client Protocol).

Currently supports [opencode](https://github.com/anomalyco/opencode).

## Features

- **Interactive REPL** — Chat with AI agents in real-time
- **One-shot mode** — Run single prompts from the command line
- **Project context** — Automatically include project files in context
- **Rich TUI** — Terminal UI with markdown rendering, syntax highlighting, and history
- **Tab completion** — Insert suggestions with Tab
- **Keyboard shortcuts** — Cancel with `Ctrl+C`, use `/` for commands, `@` for file mentions

## Requirements

- Rust stable
- [opencode](https://github.com/anomalyco/opencode) installed and in `PATH`

## Installation

```bash
cargo build --release
```

The binary will be at `target/release/mythcode`. Add it to your PATH for convenience.

## Quick Start

```bash
# Interactive REPL
mythcode

# One-shot prompt
mythcode "explain this code"

# Run against a specific project
mythcode -p ./my-project "fix the tests"
```

## Usage

### REPL Commands

| Command | Description |
|---------|-------------|
| `/exit` | Exit the application |
| `/clear` | Clear the chat history |
| `/cwd` | Show current working directory |
| `/new` | Start a new session |
| `/model` | Switch the AI model |
| `/help` | Show available commands |

### Debug Mode

```bash
mythcode --debug
```

Enable verbose logging for troubleshooting.

## Architecture

```
src/
├── main.rs          Entry point
├── cli.rs           CLI argument parsing
├── acp_client.rs    ACP protocol client
├── session.rs       Session management
├── process.rs       Process handling
├── input.rs         Input handling
├── types.rs         Shared types
└── tui/             Terminal UI components
    ├── history.rs   Chat history
    ├── input_box.rs Input field
    └── markdown.rs  Markdown rendering
```

## FAQ

### Why does it take 6-7 seconds to start?

The startup time comes from launching the ACP server and establishing the connection. This is inherent to how the ACP protocol works, not the mythcode implementation itself.

## License

MIT
