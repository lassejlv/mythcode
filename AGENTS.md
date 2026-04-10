# Mythcode — Agent Instructions

Rust TUI client for ACP-compatible coding agents. Published as `mythcode` (binary) and `@mythcode/sdk` (npm package).

## Commands

```bash
cargo check              # compile-check after every change (required)
cargo test               # run before finishing a task (required)
cargo fmt                # must pass
cargo clippy -- -D warnings  # must pass
```

No CI for lint/test — only the release workflow exists (cargo-dist + npm publish on tag push).

## Structure

**Rust binary** (`src/`) — flat module layout, one file per concern:
- `main.rs` — entrypoint, tokio `current_thread` runtime
- `cli.rs` — arg parsing, provider picker, one-shot/interactive/non-interactive modes
- `acp_client.rs` — ACP protocol client via `agent-client-protocol` crate
- `extensions.rs` — JSON-RPC bridge to extension host process
- `session.rs`, `process.rs`, `input.rs`, `spinner.rs` — supporting modules
- `types.rs` — shared types across modules
- `tui/` — TUI rendering (the only directory in `src/`), contains `render.rs`, `markdown.rs`, `input_box.rs`, `theme.rs`, etc.

**Extension system** — TypeScript extensions run in a separate host process:
- `extension-host/host.ts` — embedded at compile time, spawned as child process, bridges JSON-RPC
- `packages/sdk/` — `@mythcode/sdk` TypeScript type defs (published to npm)
- `examples/extensions/` — example extensions
- Extensions discovered from `~/.mythcode/extensions/` and `./.mythcode/extensions/`

**Website** (`website/`) — Vite + React + TanStack Router + Tailwind v4, separate package, not part of the Rust build.

## Style

- Rust edition 2024
- Group imports: std → external crates → local modules, separated by blank lines
- Avoid `.unwrap()` outside tests — use `?`
- Prefer `&str` over `String` in function args when ownership isn't needed
- Avoid unnecessary `.clone()`
- Keep files under ~300 lines; split when they grow beyond that
- No comments unless truly necessary

## Testing

11 unit tests live inline in their modules (`#[cfg(test)] mod tests` within the source files). No integration test suite. Run `cargo test` before finishing.

## Key dependencies

- `agent-client-protocol` — ACP protocol types (enabled with `unstable_session_model` and `unstable_session_resume` features)
- `ratatui` + `crossterm` — TUI rendering
- `tokio` — async runtime (`current_thread` flavor)
- `clap` — CLI args
- `syntect` — syntax highlighting
- `pulldown-cmark` — markdown rendering