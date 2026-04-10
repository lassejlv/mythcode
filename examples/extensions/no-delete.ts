import type { MythcodeAPI } from "@mythcode/sdk";

export default function activate(mc: MythcodeAPI) {
  mc.onToolCall((ctx) => {
    const title = ctx.title.toLowerCase();
    if (title.includes("delete") || title.includes("rm ") || title.includes("rm -")) {
      mc.showMessage("Blocked: delete operations are disabled by extension", "warning");
      return { allow: false, reason: "delete operations blocked" };
    }
  });
}
