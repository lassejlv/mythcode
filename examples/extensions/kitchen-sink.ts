import type { MythcodeAPI } from "@mythcode/sdk";

export default function activate(mc: MythcodeAPI) {
  let turnStart = 0;
  let turnCount = 0;
  let toolCalls = 0;
  let blockedTools = 0;

  // ── Theme ──────────────────────────────────────────────
  // Apply Tokyo Night theme on load
  mc.setTheme({
    accent: "#7aa2f7",
    green: "#9ece6a",
    red: "#f7768e",
    yellow: "#e0af68",
    magenta: "#bb9af7",
    gray: "#565f89",
    dark: "#3b4261",
    dot: "#e0af68",
    codeFg: "#9ece6a",
    codeBg: "#1a1b26",
    header1: "#7aa2f7",
    bullet: "#7aa2f7",
    thinking: "#3b4261",
    diffAddBg: "#1a2b32",
    diffDelBg: "#37222c",
  });

  // ── Status bar ─────────────────────────────────────────
  mc.setStatus("ext", "kitchen-sink loaded");

  // ── Lifecycle events ───────────────────────────────────
  mc.on("agentStart", () => {
    turnStart = Date.now();
    turnCount++;
    toolCalls = 0;
    mc.setStatus("turn", `turn #${turnCount}`);
  });

  mc.on("agentEnd", (ctx) => {
    const elapsed = ((Date.now() - turnStart) / 1000).toFixed(1);
    mc.setStatus("turn", `turn #${turnCount} · ${elapsed}s · ${ctx.stopReason}`);
  });

  mc.on("toolExecutionStart", (ctx) => {
    toolCalls++;
    mc.setStatus("tools", `${toolCalls} tool calls`);
  });

  mc.on("toolResult", (ctx) => {
    mc.setStatus("tools", `${toolCalls} tool calls`);
  });

  // ── Input interception ─────────────────────────────────
  mc.onInput((ctx) => {
    // "ping" → respond without hitting the agent
    if (ctx.text.trim() === "ping") {
      mc.showMessage("pong!");
      return { handled: true };
    }
    // "!cmd" → run a shell command directly
    if (ctx.text.startsWith("!")) {
      const cmd = ctx.text.slice(1).trim();
      if (cmd) {
        mc.exec(cmd).then((result) => {
          if (result.stdout.trim()) mc.showMessage(result.stdout.trim());
          if (result.stderr.trim()) mc.showWarning(result.stderr.trim());
        });
      }
      return { handled: true };
    }
  });

  // ── Prompt modification ────────────────────────────────
  mc.onBeforePrompt((ctx) => {
    // Prepend a system hint to every prompt
    return {
      prompt: `[You are being extended by the kitchen-sink plugin. Be concise.]\n\n${ctx.prompt}`,
    };
  });

  // ── Tool call interception ─────────────────────────────
  mc.onToolCall((ctx) => {
    const title = ctx.title.toLowerCase();
    // Block any destructive file operations
    if (title.includes("rm -rf") || title.includes("rm -r")) {
      blockedTools++;
      mc.showWarning(`Blocked: "${ctx.title}" (${blockedTools} total blocked)`);
      return { allow: false, reason: "recursive delete blocked by kitchen-sink" };
    }
  });

  // ── Commands ───────────────────────────────────────────
  mc.registerCommand({
    name: "stats",
    description: "show session stats",
    execute: async () => {
      const cwd = await mc.getCwd();
      const model = await mc.getModel();
      const counter = (await mc.state.get<number>("totalTurns")) ?? 0;
      mc.showMessage(
        [
          `cwd: ${cwd}`,
          `model: ${model}`,
          `turns this session: ${turnCount}`,
          `total turns ever: ${counter}`,
          `tool calls: ${toolCalls}`,
          `blocked: ${blockedTools}`,
        ].join(" · ")
      );
    },
  });

  mc.registerCommand({
    name: "run",
    description: "run a shell command",
    hint: "command",
    execute: async (args) => {
      if (!args.trim()) {
        mc.showWarning("usage: /run <command>");
        return;
      }
      mc.setActivity(`running: ${args}`);
      const result = await mc.exec(args);
      if (result.stdout.trim()) mc.showMessage(result.stdout.trim());
      if (result.stderr.trim()) mc.showWarning(result.stderr.trim());
      mc.setStatus("lastRun", `exit ${result.exitCode}`);
    },
  });

  mc.registerCommand({
    name: "theme-reset",
    description: "reset to default catppuccin theme",
    execute: () => {
      mc.setTheme({
        accent: "#89b4fa",
        green: "#a6e3a1",
        red: "#f38ba8",
        yellow: "#f9e2af",
        magenta: "#f5c2e7",
        gray: "#6c7086",
        dark: "#313244",
        dot: "#f9e2af",
        codeFg: "#a6e3a1",
        codeBg: "#1e3a2a",
        header1: "#89b4fa",
        bullet: "#89b4fa",
        thinking: "#585b70",
        diffAddBg: "#14322a",
        diffDelBg: "#3c1419",
      });
      mc.showMessage("Theme reset to Catppuccin Mocha");
    },
  });

  mc.registerCommand({
    name: "theme-dracula",
    description: "switch to dracula theme",
    execute: () => {
      mc.setTheme({
        accent: "#bd93f9",
        green: "#50fa7b",
        red: "#ff5555",
        yellow: "#f1fa8c",
        magenta: "#ff79c6",
        gray: "#6272a4",
        dark: "#44475a",
        dot: "#ffb86c",
        codeFg: "#50fa7b",
        codeBg: "#21222c",
        header1: "#bd93f9",
        bullet: "#bd93f9",
        thinking: "#44475a",
        diffAddBg: "#1a3a2a",
        diffDelBg: "#3a1a1a",
      });
      mc.showMessage("Theme set to Dracula");
    },
  });

  mc.registerCommand({
    name: "clear",
    description: "clear screen (from extension)",
    execute: () => mc.clearScreen(),
  });

  // ── Persist turn count across sessions ─────────────────
  mc.on("agentEnd", async () => {
    const total = ((await mc.state.get<number>("totalTurns")) ?? 0) + 1;
    await mc.state.set("totalTurns", total);
  });
}
