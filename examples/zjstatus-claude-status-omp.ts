import type { ExtensionAPI } from "@oh-my-pi/pi-coding-agent";
import { AsyncJobManager } from "@oh-my-pi/pi-coding-agent/async";

import {
  applyAsyncDetails,
  iconForAskPhase,
  iconForSessionEvent,
  iconForTurnEnd,
} from "./omp-status-logic.mts";

/**
 * omp extension: per-tab status icons for zjstatus.
 *
 * Installation:
 *   cp examples/{zjstatus-claude-status-omp.ts,omp-status-logic.mts} ~/.omp/agent/extensions/
 *   mv ~/.omp/agent/extensions/zjstatus-claude-status-omp.ts ~/.omp/agent/extensions/zellij-status.ts
 * Requires a zjstatus layout containing {claude_status}; the extension uses
 * the same pipe protocol and glyphs as zjstatus-claude-status.sh.
 */
export default function zellijStatus(pi: ExtensionAPI): void {
  pi.setLabel("zellij-status");

  // Fallback for an OMP build without an installed AsyncJobManager instance.
  // The manager is the primary source because it also knows pending deliveries.
  const runningJobs = new Set<string>();

  const send = (icon: string) => {
    pi.exec("sh", [
      "-c",
      `zellij pipe --name zjstatus -- "zjstatus::claude_status::$ZELLIJ_PANE_ID::${icon}"`,
    ]).catch(() => {});
  };


  const recordAsyncDetails = (toolName: string, details: unknown) => {
    if (!AsyncJobManager.instance() && (toolName === "bash" || toolName === "task")) {
      applyAsyncDetails(runningJobs, details);
    }
  };

  const startSession = () => {
    runningJobs.clear();
    send(iconForSessionEvent("start"));
  };

  pi.on("session_start", async () => startSession());
  pi.on("session_switch", async () => startSession());
  pi.on("turn_start", async () => send(iconForSessionEvent("thinking")));
  pi.on("auto_retry_start", async () => send(iconForSessionEvent("thinking")));

  pi.on("tool_call", async (event) => {
    if (event.toolName === "ask") send(iconForAskPhase("call"));
  });
  pi.on("tool_execution_start", async (event) => {
    if (event.toolName === "ask") send(iconForAskPhase("call"));
  });
  pi.on("tool_result", async (event) => {
    if (event.toolName === "ask") send(iconForAskPhase("result"));
    recordAsyncDetails(event.toolName, event.details);
  });
  pi.on("tool_execution_update", async (event) => {
    recordAsyncDetails(event.toolName, event.partialResult);
  });
  pi.on("turn_end", async (_event, ctx) => {
    const manager = AsyncJobManager.instance();
    send(
      iconForTurnEnd({
        runningJobCount: manager?.getRunningJobs().length ?? runningJobs.size,
        hasPendingDeliveries: manager?.hasPendingDeliveries() ?? false,
        hasPendingMessages: ctx.hasPendingMessages(),
      }),
    );
  });
  pi.on("session_shutdown", async () => {
    runningJobs.clear();
    send(iconForSessionEvent("shutdown"));
  });
}
