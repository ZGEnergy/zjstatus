import type { ExtensionAPI } from "@oh-my-pi/pi-coding-agent";

/**
 * omp extension: per-tab status icons for zjstatus.
 *
 * Mirrors the Claude Code hooks integration (zjstatus-claude-status.sh) using
 * omp's extension event system. Sends the same zjstatus pipe protocol:
 *   zellij pipe --name zjstatus -- "zjstatus::claude_status::<pane_id>::<icon>"
 *
 * State mapping (omp event → icon):
 *   🤖  session_start          session running
 *   ⏳  turn_start              agent working (prompt submitted)
 *   ⏳  auto_retry_start        agent retrying after an API error
 *   ❓  tool_call(ask)          blocking on user input
 *   ⏳  tool_result(ask)        user answered, back to thinking
 *   ✅  turn_end (idle)         turn done, no pending work
 *   ⚙   turn_end (pending)      turn done, follow-up/steer still queued
 *   ""  session_shutdown        session ended — clear icon
 *
 * The ⚙ variant uses ctx.hasPendingMessages() as the signal that work outlived
 * the turn (steers, follow-ups, nextTurn messages). This is an approximation of
 * Claude Code's background_tasks detection; it does not track async bash or the
 * job tool, which run outside the agent loop entirely. A more precise tracker
 * would intercept tool_call/tool_result for `bash` with async=true and the
 * `job` tool, but those complete out-of-band and are not surfaced as events.
 *
 * Installation:
 *   cp examples/zjstatus-claude-status-omp.ts ~/.omp/agent/extensions/zellij-status.ts
 *   # or add the path to `extensions:` in ~/.omp/agent/config.yml
 *
 * Requires: zjstatus.wasm with {claude_status} support (ZGEnergy fork) and a
 * layout with {claude_status} in the tab format (e.g. gruvbox-claude-status.kdl).
 */
export default function zellijStatus(pi: ExtensionAPI): void {
  pi.setLabel("zellij-status");

  const send = (icon: string) => {
    // pi.exec is argv-style (command, args[]) with no shell, so run through
    // `sh -c` to get $ZELLIJ_PANE_ID expansion. Unset (not inside zellij) → the
    // pipe just fails and resolves non-zero, which we ignore. pi.exec is async,
    // so a spawn error rejects the promise — .catch() swallows it (a try/catch
    // around a non-awaited call would miss it), never crashing the session.
    pi.exec("sh", [
      "-c",
      `zellij pipe --name zjstatus -- "zjstatus::claude_status::$ZELLIJ_PANE_ID::${icon}"`,
    ]).catch(() => {});
  };

  // 🤖 session running
  pi.on("session_start", async () => send("🤖"));

  // ⏳ agent working (prompt submitted or resumed after a question)
  pi.on("turn_start", async () => send("⏳"));

  // ⏳ agent retrying after an API error
  pi.on("auto_retry_start", async () => send("⏳"));

  // ❓ blocking on user input (ask tool is about to execute)
  pi.on("tool_call", async (event) => {
    if (event.toolName === "ask") send("❓");
  });

  // ⏳ user answered, back to thinking
  pi.on("tool_result", async (event) => {
    if (event.toolName === "ask") send("⏳");
  });

  // ✅ turn done, or ⚙ if pending work is still queued
  pi.on("turn_end", async (_event, ctx) => {
    send(ctx.hasPendingMessages() ? "⚙" : "✅");
  });

  // clear icon when session ends
  pi.on("session_shutdown", async () => send(""));
}
