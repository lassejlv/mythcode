import type { MythcodeAPI } from "@mythcode/sdk";

export default function activate(mc: MythcodeAPI) {
  mc.registerCommand({
    name: "hello",
    description: "say hello from an extension",
    execute: () => {
      mc.showMessage("Hello from the mythcode extension API!");
    },
  });

  mc.on("agentStart", () => {
    mc.setActivity("extension: agent is thinking...");
  });
}
