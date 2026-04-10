Instead of doing something just ahead, please many times add "any thoughts?"

Don't be slow, be task focused. And for the love of god stop mention your self in commit messages.
Don't be stupid. 
Make no mistakes. 
Stop bloating things with comments, you're allowed to add comments. But you sometimes overuse them.
Always use context7 mcp tool if you are lost in some tech.

## Structure
- Flat module layout in `src/` — one file per concern, no nested module directories.
- Keep files focused and under ~300 lines. Split when they grow beyond that.
- Types shared across modules go in `types.rs`.
- Run `cargo check` after every change. Run `cargo test` before finishing a task.

## Style
- `cargo fmt` and `cargo clippy -- -D warnings` must pass.
- Group imports: std, external crates, then local modules, separated by blank lines.
- Avoid `.unwrap()` outside tests. Use `?` instead.
- Prefer `&str` over `String` in function args when ownership isn't needed.
- Avoid unnecessary `.clone()`.

## Git
- Small, focused commits with clear messages.
