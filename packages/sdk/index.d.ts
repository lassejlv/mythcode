export interface MythcodeAPI {
  // Lifecycle events
  on(event: "sessionStart", handler: (ctx: SessionContext) => void): Disposable;
  on(event: "sessionEnd", handler: (ctx: Record<string, unknown>) => void): Disposable;
  on(event: "agentStart", handler: (ctx: Record<string, unknown>) => void): Disposable;
  on(event: "agentEnd", handler: (ctx: AgentEndContext) => void): Disposable;
  on(event: "toolResult", handler: (ctx: ToolResultContext) => void): Disposable;
  on(event: "toolExecutionStart", handler: (ctx: ToolExecutionContext) => void): Disposable;
  on(event: "toolExecutionEnd", handler: (ctx: ToolExecutionContext) => void): Disposable;

  // Interception hooks
  onInput(handler: (ctx: InputContext) => InputResult | Promise<InputResult>): Disposable;
  onBeforePrompt(handler: (ctx: PromptContext) => PromptResult | Promise<PromptResult>): Disposable;
  onToolCall(handler: (ctx: ToolCallContext) => ToolCallResult | Promise<ToolCallResult>): Disposable;

  // Registration
  registerCommand(def: CommandDefinition): Disposable;
  registerTool(def: ToolDefinition): Disposable;

  // UI
  showMessage(text: string, level?: "info" | "warning"): void;
  showWarning(text: string): void;
  setActivity(text: string): void;
  clearScreen(): void;
  setStatus(key: string, text?: string): void;
  removeStatus(key: string): void;

  // Session control
  exit(): void;
  newSession(): Promise<void>;
  getCwd(): Promise<string>;
  getModel(): Promise<string | null>;
  setModel(modelId: string): Promise<void>;

  // Messaging
  sendMessage(text: string): void;
  sendUserMessage(text: string): void;

  // Shell execution
  exec(command: string): Promise<ExecResult>;

  // Theme
  setTheme(colors: ThemeColors): void;
  getTheme(): Promise<ThemeColors>;

  // State persistence
  state: StateAPI;

  // Metadata
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

export interface ToolExecutionContext {
  toolCallId: string;
  title: string;
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

export interface ExecResult {
  stdout: string;
  stderr: string;
  exitCode: number;
}

export interface StateAPI {
  get<T = unknown>(key: string): Promise<T | undefined>;
  set(key: string, value: unknown): Promise<void>;
}

export interface ThemeColors {
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

export interface Disposable {
  dispose(): void;
}
