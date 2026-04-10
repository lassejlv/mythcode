# Mythcode Extension System

Mythcode supports extensions that add custom commands, intercept user input and tool calls, modify the UI, and persist state. Extensions are TypeScript/JavaScript files that run in a separate host process and communicate with the Rust TUI over JSON-RPC.

## Architecture

```
┌──────────────┐   stdin/stdout    ┌──────────────────┐
│  Rust TUI    │ ◄── JSON-RPC ──► │  Extension Host   │
│ (mythcode)   │                   │  (bun / npx tsx)  │
└──────────────┘                   │                   │
                                   │  ┌─────────────┐  │
                                   │  │ Extension A  │  │
                                   │  ├─────────────┤  │
                                   │  │ Extension B  │  │
                                   │  └─────────────┘  │
                                   └──────────────────┘
```

The Rust binary embeds `extension-host/host.ts` at compile time, writes it to a temp file, and spawns it as a child process. The host loads each extension, creates a sandboxed `MythcodeAPI` instance for it, and bridges API calls to JSON-RPC messages.

## Getting Started

### Extension Locations

Extensions are discovered from two directories:

- **Global**: `~/.mythcode/extensions/`
- **Project-local**: `./.mythcode/extensions/`

### File Structure

An extension can be either:

- A single file: `my-extension.ts` or `my-extension.js`
- A directory with an entry point: `my-extension/index.ts`

### Entry Point

Every extension must export an `activate` function (default or named) that receives the API:

```typescript
import type { MythcodeAPI } from "@mythcode/sdk";

export default function activate(mc: MythcodeAPI) {
  mc.showMessage("Extension loaded!");
}
```

### Runtime

The host tries `bun` first, then falls back to `npx tsx`. Install one of them.

---

## API Reference

All methods are available on the `MythcodeAPI` object passed to `activate()`.

### Lifecycle Events

Subscribe to events with `mc.on(event, handler)`. Returns a `Disposable` — call `.dispose()` to unsubscribe.

| Event | Handler Signature | Fires When |
|---|---|---|
| `sessionStart` | `(ctx: SessionContext) => void` | A new agent session begins |
| `sessionEnd` | `(ctx: Record<string, unknown>) => void` | The session ends |
| `agentStart` | `(ctx: Record<string, unknown>) => void` | The agent starts processing |
| `agentEnd` | `(ctx: AgentEndContext) => void` | The agent finishes a turn |
| `toolResult` | `(ctx: ToolResultContext) => void` | A tool returns a result |
| `toolExecutionStart` | `(ctx: ToolExecutionContext) => void` | A tool begins executing |
| `toolExecutionEnd` | `(ctx: ToolExecutionContext) => void` | A tool finishes executing |

### Interception Hooks

These hooks can modify or block behavior. Return a value to intervene, or return nothing to let it pass through.

#### `onInput(handler)`

Intercept user input before it reaches the agent.

```typescript
mc.onInput((ctx: InputContext) => {
  // Modify the text
  return { text: ctx.text.toUpperCase() };
  // Or handle it entirely (don't send to agent)
  return { handled: true };
  // Or let it pass through
  return undefined;
});
```

#### `onBeforePrompt(handler)`

Modify or skip the prompt before it's sent.

```typescript
mc.onBeforePrompt((ctx: PromptContext) => {
  return { prompt: `[prefix]\n\n${ctx.prompt}` };
  // Or skip sending entirely
  return { skip: true };
});
```

#### `onToolCall(handler)`

Allow or block tool calls.

```typescript
mc.onToolCall((ctx: ToolCallContext) => {
  if (ctx.title.includes("delete")) {
    return { allow: false, reason: "blocked by extension" };
  }
  return { allow: true };
});
```

### Registration

#### `registerCommand(def)`

Register a slash command accessible via `/name` in the TUI.

```typescript
mc.registerCommand({
  name: "hello",
  description: "Say hello",
  hint: "<name>",  // optional argument hint
  execute: (args: string) => {
    mc.showMessage(`Hello, ${args || "world"}!`);
  },
});
```

#### `registerTool(def)`

Register a custom tool the agent can invoke.

```typescript
mc.registerTool({
  name: "myTool",
  description: "Does something useful",
  inputSchema: {
    type: "object",
    properties: {
      query: { type: "string" },
    },
  },
  execute: async (input) => {
    return { content: `Result for ${input.query}` };
    // Or on error:
    return { content: "something went wrong", isError: true };
  },
});
```

### UI Actions

| Method | Description |
|---|---|
| `showMessage(text, level?)` | Show a message. `level` is `"info"` (default) or `"warning"` |
| `showWarning(text)` | Shorthand for `showMessage(text, "warning")` |
| `setActivity(text)` | Set the activity/status text shown during processing |
| `clearScreen()` | Clear the TUI screen |
| `setStatus(key, text?)` | Set a key-value pair in the status bar |
| `removeStatus(key)` | Remove a status bar entry |

### Session Control

| Method | Returns | Description |
|---|---|---|
| `exit()` | `void` | Exit the application |
| `newSession()` | `Promise<void>` | Start a new session |
| `getCwd()` | `Promise<string>` | Get the current working directory |
| `getModel()` | `Promise<string \| null>` | Get the active model ID |
| `setModel(modelId)` | `Promise<void>` | Switch the active model |

### Messaging

| Method | Description |
|---|---|
| `sendMessage(text)` | Send a message as the agent |
| `sendUserMessage(text)` | Send a message as the user |

### Shell Execution

```typescript
const result = await mc.exec("ls -la");
// result: { stdout: string, stderr: string, exitCode: number }
```

### State Persistence

State is saved per-extension at `~/.mythcode/state/{extensionName}.json`.

```typescript
const count = (await mc.state.get<number>("counter")) ?? 0;
await mc.state.set("counter", count + 1);
```

### Theme Customization

Override TUI colors. All fields are optional hex color strings (`#RRGGBB`).

```typescript
mc.setTheme({
  accent: "#7aa2f7",
  green: "#9ece6a",
  red: "#f7768e",
  yellow: "#e0af68",
  magenta: "#bb9af7",
  gray: "#565f89",
  dark: "#1a1b26",
  dot: "#565f89",
  codeFg: "#a9b1d6",
  codeBg: "#1a1b26",
  header1: "#7aa2f7",
  bullet: "#7aa2f7",
  thinking: "#565f89",
  diffAddBg: "#283B4D",
  diffDelBg: "#3B2839",
});
```

### Extension Metadata

```typescript
mc.extension.name; // Extension filename or directory name
mc.extension.dir;  // Extension directory path
```

---

## Types

```typescript
interface SessionContext {
  sessionId: string;
  cwd: string;
  provider: string;
}

interface AgentEndContext {
  stopReason: string;
  elapsed?: number;
}

interface InputContext {
  text: string;
}

type InputResult = { text: string } | { handled: true } | void;

interface PromptContext {
  prompt: string;
}

type PromptResult = { prompt: string } | { skip: true } | void;

interface ToolCallContext {
  toolCallId: string;
  title: string;
  kind: string;
  content: unknown[];
}

type ToolCallResult = { allow: true } | { allow: false; reason?: string } | void;

interface ToolResultContext {
  toolCallId: string;
  title: string;
  content: unknown[];
}

interface ToolExecutionContext {
  toolCallId: string;
  title: string;
}

interface CommandDefinition {
  name: string;
  description: string;
  hint?: string;
  execute: (args: string) => void | Promise<void>;
}

interface ToolDefinition {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  execute: (input: Record<string, unknown>) => Promise<ToolOutput>;
}

interface ToolOutput {
  content: string;
  isError?: boolean;
}

interface ExecResult {
  stdout: string;
  stderr: string;
  exitCode: number;
}

interface StateAPI {
  get<T = unknown>(key: string): Promise<T | undefined>;
  set(key: string, value: unknown): Promise<void>;
}

interface ThemeColors {
  accent?: string;
  green?: string;
  red?: string;
  yellow?: string;
  magenta?: string;
  gray?: string;
  dark?: string;
  dot?: string;
  codeFg?: string;
  codeBg?: string;
  header1?: string;
  bullet?: string;
  thinking?: string;
  diffAddBg?: string;
  diffDelBg?: string;
}

interface Disposable {
  dispose(): void;
}
```

---

## Examples

### Hello World

```typescript
import type { MythcodeAPI } from "@mythcode/sdk";

export default function activate(mc: MythcodeAPI) {
  mc.registerCommand({
    name: "hello",
    description: "say hello from an extension",
    execute: () => {
      mc.showMessage("Hello from the mythcode extension API!");
    },
  });
}
```

### Input Interception — Quick Shell Commands

```typescript
export default function activate(mc: MythcodeAPI) {
  mc.onInput((ctx) => {
    if (ctx.text.startsWith("!")) {
      const cmd = ctx.text.slice(1).trim();
      mc.exec(cmd).then((result) => {
        if (result.stdout.trim()) mc.showMessage(result.stdout.trim());
        if (result.stderr.trim()) mc.showWarning(result.stderr.trim());
      });
      return { handled: true };
    }
  });
}
```

### Block Dangerous Tool Calls

```typescript
export default function activate(mc: MythcodeAPI) {
  mc.onToolCall((ctx) => {
    const title = ctx.title.toLowerCase();
    if (title.includes("delete") || title.includes("rm ")) {
      mc.showMessage("Blocked: delete operations disabled", "warning");
      return { allow: false, reason: "delete operations blocked" };
    }
  });
}
```

### Track Agent Turns with Persistent State

```typescript
export default function activate(mc: MythcodeAPI) {
  mc.on("agentEnd", async () => {
    const total = ((await mc.state.get<number>("totalTurns")) ?? 0) + 1;
    await mc.state.set("totalTurns", total);
    mc.setStatus("turns", `${total} turns`);
  });
}
```

---

## JSON-RPC Protocol Reference

For contributors working on `src/extensions.rs` or `extension-host/host.ts`. All messages are line-delimited JSON-RPC 2.0 over stdin/stdout.

### Host -> Rust

| Method | Type | Params | Response |
|---|---|---|---|
| `register/command` | request | `{ name, description, hint? }` | `true` |
| `register/tool` | request | `{ name, description, inputSchema }` | `true` |
| `action/showMessage` | notification | `{ text, level }` | — |
| `action/setActivity` | notification | `{ text }` | — |
| `action/clearScreen` | notification | `{}` | — |
| `action/setStatus` | notification | `{ key, value }` | — |
| `action/setTheme` | request | `ThemeColors` | `true` |
| `action/exit` | notification | `{}` | — |
| `action/newSession` | request | `{}` | `true` |
| `action/getCwd` | request | `{}` | `string` |
| `action/getModel` | request | `{}` | `string \| null` |
| `action/setModel` | request | `{ modelId }` | `true` |
| `action/sendMessage` | notification | `{ text }` | — |
| `action/sendUserMessage` | notification | `{ text }` | — |
| `action/exec` | request | `{ command }` | `{ stdout, stderr, exitCode }` |
| `host/ready` | notification | `{}` | — |
| `host/error` | notification | `{ extension, error }` | — |

### Rust -> Host

| Method | Type | Params | Expected Response |
|---|---|---|---|
| `command/execute` | request | `{ name, args }` | — |
| `lifecycle/sessionStart` | notification | `SessionContext` | — |
| `lifecycle/sessionEnd` | notification | `{}` | — |
| `lifecycle/agentStart` | notification | `{}` | — |
| `lifecycle/agentEnd` | notification | `{ stopReason, elapsed? }` | — |
| `lifecycle/toolResult` | notification | `ToolResultContext` | — |
| `lifecycle/toolExecutionStart` | notification | `ToolExecutionContext` | — |
| `lifecycle/toolExecutionEnd` | notification | `ToolExecutionContext` | — |
| `lifecycle/input` | request | `{ text }` | `InputResult` |
| `lifecycle/beforePrompt` | request | `{ prompt }` | `PromptResult` |
| `lifecycle/toolCall` | request | `ToolCallContext` | `ToolCallResult` |
| `lifecycle/shutdown` | notification | `{}` | — |
