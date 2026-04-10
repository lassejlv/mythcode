#!/usr/bin/env bun

import { readFileSync, writeFileSync, mkdirSync, existsSync } from "fs";
import { join, basename } from "path";
import { homedir } from "os";

// Types inlined to avoid runtime dependency on @mythcode/sdk
interface Disposable { dispose(): void }
interface InputContext { text: string }
type InputResult = { text: string } | { handled: true } | void
interface PromptContext { prompt: string }
type PromptResult = { prompt: string } | { skip: true } | void
interface ToolCallContext { toolCallId: string; title: string; kind: string; content: unknown[] }
type ToolCallResult = { allow: true } | { allow: false; reason?: string } | void
interface ToolExecutionContext { toolCallId: string; title: string }
interface ExecResult { stdout: string; stderr: string; exitCode: number }
interface CommandDefinition { name: string; description: string; hint?: string; execute: (args: string) => void | Promise<void> }
interface StateAPI { get<T = unknown>(key: string): Promise<T | undefined>; set(key: string, value: unknown): Promise<void> }
interface MythcodeAPI {
  on(event: string, handler: (ctx: any) => void): Disposable
  onInput(handler: (ctx: InputContext) => InputResult | Promise<InputResult>): Disposable
  onBeforePrompt(handler: (ctx: PromptContext) => PromptResult | Promise<PromptResult>): Disposable
  onToolCall(handler: (ctx: ToolCallContext) => ToolCallResult | Promise<ToolCallResult>): Disposable
  registerCommand(def: CommandDefinition): Disposable
  registerTool(def: { name: string; description: string; inputSchema: Record<string, unknown>; execute: (input: Record<string, unknown>) => Promise<{ content: string; isError?: boolean }> }): Disposable
  showMessage(text: string, level?: "info" | "warning"): void
  showWarning(text: string): void
  setActivity(text: string): void
  clearScreen(): void
  exit(): void
  newSession(): Promise<void>
  getCwd(): Promise<string>
  getModel(): Promise<string | null>
  setModel(modelId: string): Promise<void>
  sendMessage(text: string): void
  sendUserMessage(text: string): void
  exec(command: string): Promise<ExecResult>
  state: StateAPI
  extension: { name: string; dir: string }
}

// JSON-RPC over stdin/stdout

let nextId = 1;
const pendingRequests = new Map<number, { resolve: (v: any) => void; reject: (e: any) => void }>();

function send(msg: any) {
  process.stdout.write(JSON.stringify(msg) + "\n");
}

function sendNotification(method: string, params?: any) {
  send({ jsonrpc: "2.0", method, params: params ?? {} });
}

function sendRequest(method: string, params?: any): Promise<any> {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pendingRequests.set(id, { resolve, reject });
    send({ jsonrpc: "2.0", id, method, params: params ?? {} });
  });
}

function sendResponse(id: number, result: any) {
  send({ jsonrpc: "2.0", id, result });
}

// State persistence
const stateDir = join(homedir(), ".mythcode", "state");

function ensureStateDir() {
  if (!existsSync(stateDir)) {
    mkdirSync(stateDir, { recursive: true });
  }
}

function stateFile(extName: string) {
  return join(stateDir, `${extName}.json`);
}

function loadState(extName: string): Record<string, any> {
  const file = stateFile(extName);
  if (!existsSync(file)) return {};
  try {
    return JSON.parse(readFileSync(file, "utf-8"));
  } catch {
    return {};
  }
}

function saveState(extName: string, state: Record<string, any>) {
  ensureStateDir();
  writeFileSync(stateFile(extName), JSON.stringify(state, null, 2));
}

// Extension registry
interface ExtensionEntry {
  name: string;
  path: string;
  api: MythcodeAPI;
  inputHandlers: Array<(ctx: InputContext) => InputResult | Promise<InputResult>>;
  promptHandlers: Array<(ctx: PromptContext) => PromptResult | Promise<PromptResult>>;
  toolCallHandlers: Array<(ctx: ToolCallContext) => ToolCallResult | Promise<ToolCallResult>>;
  lifecycleHandlers: Map<string, Array<(ctx: any) => void>>;
  commands: Map<string, CommandDefinition>;
  disposables: Array<Disposable>;
}

const extensions: ExtensionEntry[] = [];

function makeDisposable(ext: ExtensionEntry, arr: any[], item: any): Disposable {
  const d = { dispose: () => { const i = arr.indexOf(item); if (i >= 0) arr.splice(i, 1); } };
  ext.disposables.push(d);
  return d;
}

function createAPI(ext: ExtensionEntry): MythcodeAPI {
  const state = loadState(ext.name);

  return {
    on(event: string, handler: (ctx: any) => void): Disposable {
      const handlers = ext.lifecycleHandlers.get(event) ?? [];
      handlers.push(handler);
      ext.lifecycleHandlers.set(event, handlers);
      return makeDisposable(ext, handlers, handler);
    },
    onInput(handler) {
      ext.inputHandlers.push(handler);
      return makeDisposable(ext, ext.inputHandlers, handler);
    },
    onBeforePrompt(handler) {
      ext.promptHandlers.push(handler);
      return makeDisposable(ext, ext.promptHandlers, handler);
    },
    onToolCall(handler) {
      ext.toolCallHandlers.push(handler);
      return makeDisposable(ext, ext.toolCallHandlers, handler);
    },
    registerCommand(def) {
      ext.commands.set(def.name, def);
      sendRequest("register/command", { name: def.name, description: def.description, hint: def.hint });
      return makeDisposable(ext, [...ext.commands.values()], def);
    },
    registerTool(def) {
      sendRequest("register/tool", { name: def.name, description: def.description, inputSchema: def.inputSchema });
      return { dispose: () => {} };
    },

    // UI
    showMessage(text, level) {
      sendNotification("action/showMessage", { text, level: level ?? "info" });
    },
    showWarning(text) {
      sendNotification("action/showMessage", { text, level: "warning" });
    },
    setActivity(text) {
      sendNotification("action/setActivity", { text });
    },
    clearScreen() {
      sendNotification("action/clearScreen");
    },

    // Session control
    exit() {
      sendNotification("action/exit");
    },
    async newSession() {
      await sendRequest("action/newSession");
    },
    async getCwd() {
      return await sendRequest("action/getCwd");
    },
    async getModel() {
      return await sendRequest("action/getModel");
    },
    async setModel(modelId: string) {
      await sendRequest("action/setModel", { modelId });
    },

    // Messaging
    sendMessage(text) {
      sendNotification("action/sendMessage", { text });
    },
    sendUserMessage(text) {
      sendNotification("action/sendUserMessage", { text });
    },

    // Shell execution
    async exec(command: string): Promise<ExecResult> {
      return await sendRequest("action/exec", { command });
    },

    setTheme(colors: Record<string, string>) {
      sendRequest("action/setTheme", colors);
    },
    async getTheme(): Promise<Record<string, string>> {
      return await sendRequest("action/getTheme");
    },

    // State
    state: {
      async get<T = any>(key: string): Promise<T | undefined> {
        return state[key] as T | undefined;
      },
      async set(key: string, value: any): Promise<void> {
        state[key] = value;
        saveState(ext.name, state);
      },
    },

    extension: { name: ext.name, dir: ext.path },
  };
}

// Load extensions from argv
async function loadExtensions() {
  const paths = process.argv.slice(2);

  for (const extPath of paths) {
    const name = basename(extPath).replace(/\.(ts|js)$/, "");
    const ext: ExtensionEntry = {
      name,
      path: extPath,
      api: null as any,
      inputHandlers: [],
      promptHandlers: [],
      toolCallHandlers: [],
      lifecycleHandlers: new Map(),
      commands: new Map(),
      disposables: [],
    };
    ext.api = createAPI(ext);

    try {
      const mod = await import(extPath);
      const activate = mod.default ?? mod.activate;
      if (typeof activate === "function") {
        await activate(ext.api);
      }
      extensions.push(ext);
    } catch (err: any) {
      sendNotification("host/error", { extension: name, error: err.message ?? String(err) });
    }
  }

  sendNotification("host/ready", {
    extensions: extensions.map((e) => ({ name: e.name })),
  });
}

function dispatchLifecycle(event: string, params: any) {
  for (const ext of extensions) {
    const handlers = ext.lifecycleHandlers.get(event) ?? [];
    for (const handler of handlers) {
      try { handler(params); } catch (err: any) {
        sendNotification("host/error", { extension: ext.name, error: err.message });
      }
    }
  }
}

// Handle incoming messages from Rust
async function handleMessage(msg: any) {
  // Response to a request we sent
  if (msg.id != null && (msg.result !== undefined || msg.error !== undefined)) {
    const pending = pendingRequests.get(msg.id);
    if (pending) {
      pendingRequests.delete(msg.id);
      if (msg.error) pending.reject(msg.error);
      else pending.resolve(msg.result);
    }
    return;
  }

  const method = msg.method as string;
  const params = msg.params ?? {};
  const id = msg.id as number | undefined;

  switch (method) {
    case "lifecycle/sessionStart":
    case "lifecycle/sessionEnd":
    case "lifecycle/agentStart":
    case "lifecycle/agentEnd":
    case "lifecycle/toolResult":
    case "lifecycle/toolExecutionStart":
    case "lifecycle/toolExecutionEnd": {
      dispatchLifecycle(method.split("/")[1], params);
      break;
    }

    case "lifecycle/input": {
      let result: any = {};
      for (const ext of extensions) {
        for (const handler of ext.inputHandlers) {
          try {
            const r = await handler({ text: params.text });
            if (r) { result = r; break; }
          } catch (err: any) {
            sendNotification("host/error", { extension: ext.name, error: err.message });
          }
        }
        if (result.handled || result.text) break;
      }
      if (id != null) sendResponse(id, result);
      break;
    }

    case "lifecycle/beforePrompt": {
      let result: any = {};
      for (const ext of extensions) {
        for (const handler of ext.promptHandlers) {
          try {
            const r = await handler({ prompt: params.prompt });
            if (r) { result = r; break; }
          } catch (err: any) {
            sendNotification("host/error", { extension: ext.name, error: err.message });
          }
        }
        if (result.skip || result.prompt) break;
      }
      if (id != null) sendResponse(id, result);
      break;
    }

    case "lifecycle/toolCall": {
      let result: any = { allow: true };
      for (const ext of extensions) {
        for (const handler of ext.toolCallHandlers) {
          try {
            const r = await handler(params);
            if (r && r.allow === false) { result = r; break; }
          } catch (err: any) {
            sendNotification("host/error", { extension: ext.name, error: err.message });
          }
        }
        if (result.allow === false) break;
      }
      if (id != null) sendResponse(id, result);
      break;
    }

    case "command/execute": {
      const { name, args } = params;
      for (const ext of extensions) {
        const cmd = ext.commands.get(name);
        if (cmd) {
          try { await cmd.execute(args ?? ""); } catch (err: any) {
            sendNotification("host/error", { extension: ext.name, error: err.message });
          }
          break;
        }
      }
      if (id != null) sendResponse(id, true);
      break;
    }

    case "lifecycle/shutdown": {
      process.exit(0);
    }
  }
}

// Read stdin line by line
async function readLoop() {
  const reader = require("readline").createInterface({ input: process.stdin });
  for await (const line of reader) {
    if (!line.trim()) continue;
    try {
      const msg = JSON.parse(line);
      handleMessage(msg);
    } catch {}
  }
}

loadExtensions().then(() => readLoop());
