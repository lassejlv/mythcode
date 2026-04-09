# mythcode

A fast terminal client for [ACP](https://agentclientprotocol.com)-compatible coding agents.

## Install

```bash
npm install -g @mythcode/cli
```

Or download binaries from the [latest release](https://github.com/lassejlv/minicode/releases).

## Usage

```bash
mythcode                              # interactive TUI
mythcode "explain this function"      # one-shot
mythcode -p ./my-app "fix the tests"  # scoped to a directory
mythcode --provider claude            # skip provider picker
```

## License

MIT
