# Mythcode

> A fast Rust CLI for talking to ACP-compatible coding agents from your terminal.

Mythcode gives you a lightweight local client for agent workflows without dragging you into a heavy editor integration. It supports interactive chat, one-shot prompts, project-scoped sessions, and a terminal UI that renders markdown, diffs, plans, tool output, and permission prompts cleanly.

## What It Supports

- Interactive TUI for back-and-forth agent sessions
- One-shot prompt mode for quick commands
- Project-scoped sessions with file indexing
- ACP providers: `opencode`, `codex`, `claude`, and `pi`
- Markdown rendering with ANSI-aware wrapping
- Syntax-highlighted diffs and tool output previews
- Session resume support
- Model switching and mode switching from inside the TUI
- File mentions with `@`
- Slash commands for common local actions

## Requirements

- Rust stable toolchain
- A working ACP provider

Provider setup:

- `opencode`: install [`opencode`](https://github.com/anomalyco/opencode) and make sure `opencode` is in your `PATH`
- `codex`: Mythcode launches `npx -y @zed-industries/codex-acp`
- `claude`: Mythcode launches `npx -y @agentclientprotocol/claude-agent-acp`
- `pi`: Mythcode launches `npx -y pi-acp`

If you use `codex`, `claude`, or `pi`, you need a working `npx` environment available in your shell.

## Installation

Global install from npm:

```bash
npm install -g @mythcode/cli
mythcode --help
```

The npm package is published as `@mythcode/cli`, but it still exposes the `mythcode` command. The published package is a `cargo-dist` installer, so it installs the correct native Rust binary for your platform instead of wrapping the app in JavaScript.

Build locally from source:

```bash
cargo build --release
```

The compiled binary will be at `target/release/mythcode`.

If you want it globally available:

```bash
cargo install --path .
```

## GitHub Release Flow

The release workflow lives at [`.github/workflows/release.yml`](./.github/workflows/release.yml). It uses `cargo-dist` to:

- build native release artifacts for Linux, macOS Intel, macOS Apple Silicon, and Windows
- run when a GitHub Release is created for a version tag like `v0.1.0`
- publish the npm package as public `@mythcode/cli`

Required one-time setup:

- create or reserve the npm scope/package you want to publish to, and make sure the token you use can publish `@mythcode/cli`
- add `NPM_TOKEN` as a GitHub Actions secret in the repository
- if this repo is moved or renamed, update the `repository` field in [`Cargo.toml`](./Cargo.toml) so `cargo-dist` points installers at the right GitHub Release URL
- make sure the GitHub Release tag matches the version in [`Cargo.toml`](./Cargo.toml); the workflow fails fast if they differ

Release steps:

```bash
# bump Cargo.toml version first
cargo check
git add Cargo.toml Cargo.lock README.md .github/workflows/release.yml
git commit -m "release: v0.1.0"
git push origin main
git tag v0.1.0
git push origin v0.1.0
gh release create v0.1.0 --verify-tag --title "v0.1.0"
```

Once the workflow finishes, installing with `npm install -g @mythcode/cli` should put `mythcode` on your `PATH`.

## Quick Start

Interactive mode:

```bash
mythcode
```

One-shot prompt:

```bash
mythcode "explain this codebase"
```

Target a specific project:

```bash
mythcode --project ./my-project "fix the failing tests"
```

Pick a provider explicitly:

```bash
mythcode --provider codex
mythcode --provider claude
mythcode --provider opencode
mythcode --provider pi
```

Choose a model up front:

```bash
mythcode --provider codex --model gpt-5.4
```

## Usage

```bash
mythcode [OPTIONS] [PROMPT]...
```

Options:

- `-p, --project <PATH>`: run against a specific working directory
- `--model <MODEL>`: request a specific model from the ACP server
- `--provider <PROVIDER>`: choose `opencode`, `codex`, `claude`, or `pi`
- `--debug`: enable verbose protocol/debug output

Behavior:

- No prompt + interactive terminal: launches the TUI
- Prompt provided: runs a one-shot prompt and prints the response
- Piped or non-interactive stdin: runs line-by-line non-interactive prompting

## TUI Controls

Core controls:

- `Enter`: submit the current input
- `Shift+Enter` or `Alt+Enter`: insert a newline
- `Ctrl+C`: cancel current turn, press again to exit
- `Ctrl+D`: exit
- `Ctrl+W`: delete previous word
- `Ctrl+U`: clear input
- `Ctrl+O`: expand the latest tool output preview
- `Shift+Tab`: cycle agent modes when the provider exposes multiple modes

Autocomplete and mentions:

- `@`: mention files from the indexed project
- `/`: open slash-command completion
- `Tab` / `Down`: cycle suggestions
- `Up`: move backward through suggestions
- `Enter`: accept the selected suggestion without sending
- `Esc`: close suggestions or selection UI

Selection screens:

- `Up` / `Down`: move selection
- `Enter`: confirm
- `Esc`: cancel

## Slash Commands

Local commands currently implemented:

- `/help`: show local commands
- `/model`: change the active model
- `/new`: start a fresh session
- `/cwd`: print the current working directory
- `/clear`: clear terminal history
- `/resume`: resume a previous session
- `/exit`: quit Mythcode

## Architecture

High-level layout:

```text
src/
├── main.rs
├── cli.rs
├── acp_client.rs
├── process.rs
├── session.rs
├── input.rs
├── spinner.rs
├── types.rs
└── tui/
```

Key pieces:

- `src/cli.rs`: argument parsing and runtime entry flow
- `src/acp_client.rs`: ACP client integration
- `src/process.rs`: provider process spawning and transport wiring
- `src/session.rs`: session state and lifecycle
- `src/input.rs`: file indexing and input helpers
- `src/tui/`: terminal UI, rendering, history, keyboard handling, and highlighting

## Notes

- Startup latency depends heavily on the ACP provider process booting and connecting
- `opencode` is launched directly, while the other providers are currently launched through `npx`
- If a provider fails during startup, Mythcode surfaces stderr context to make debugging less miserable

## License

MIT
