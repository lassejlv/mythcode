export interface MythcodeAPI {
  on(event: "sessionStart", handler: (ctx: SessionContext) => void): Disposable;
  on(event: "agentStart", handler: (ctx: Record<string, unknown>) => void): Disposable;
  on(event: "agentEnd", handler: (ctx: AgentEndContext) => void): Disposable;
  on(event: "toolResult", handler: (ctx: ToolResultContext) => void): Disposable;

  onInput(handler: (ctx: InputContext) => InputResult | Promise<InputResult>): Disposable;
  onBeforePrompt(handler: (ctx: PromptContext) => PromptResult | Promise<PromptResult>): Disposable;
  onToolCall(handler: (ctx: ToolCallContext) => ToolCallResult | Promise<ToolCallResult>): Disposable;

  registerCommand(def: CommandDefinition): Disposable;
  registerTool(def: ToolDefinition): Disposable;

  showMessage(text: string, level?: "info" | "warning"): void;
  setActivity(text: string): void;

  state: StateAPI;
  extension: { name: string; dir: string };
}

export interface SessionContext {
  sessionId: string;
  cwd: string;
  provider: string;
}

export interface AgentEndContext {
  stopReason: string;
  elapsed?: number;
}

export interface ToolResultContext {
  toolCallId: string;
  title: string;
  content: unknown[];
}

export interface InputContext {
  text: string;
}

export type InputResult = { text: string } | { handled: true } | void;

export interface PromptContext {
  prompt: string;
}

export type PromptResult = { prompt: string } | { skip: true } | void;

export interface ToolCallContext {
  toolCallId: string;
  title: string;
  kind: string;
  content: unknown[];
}

export type ToolCallResult = { allow: true } | { allow: false; reason?: string } | void;

export interface CommandDefinition {
  name: string;
  description: string;
  hint?: string;
  execute: (args: string) => void | Promise<void>;
}

export interface ToolDefinition {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  execute: (input: Record<string, unknown>) => Promise<ToolOutput>;
}

export interface ToolOutput {
  content: string;
  isError?: boolean;
}

export interface StateAPI {
  get<T = unknown>(key: string): Promise<T | undefined>;
  set(key: string, value: unknown): Promise<void>;
}

export interface Disposable {
  dispose(): void;
}
