export type AskPhase = "call" | "result";
export type SessionEvent = "start" | "thinking" | "shutdown";

export interface TurnEndState {
  runningJobCount: number;
  hasPendingDeliveries: boolean;
  hasPendingMessages: boolean;
}

type AsyncDetails = {
  async?: {
    state?: string;
    jobId?: string;
    type?: string;
  };
};

export function detailsFromExecutionUpdate(partialResult: unknown): unknown {
  if (typeof partialResult !== "object" || partialResult === null || !("details" in partialResult)) {
    return undefined;
  }

  return (partialResult as { details?: unknown }).details;
}

export function iconForAskPhase(phase: AskPhase): "❓" | "⏳" {
  return phase === "call" ? "❓" : "⏳";
}

export function iconForSessionEvent(event: SessionEvent): "🤖" | "⏳" | "" {
  switch (event) {
    case "start":
      return "🤖";
    case "thinking":
      return "⏳";
    case "shutdown":
      return "";
  }
}

export function iconForTurnEnd(state: TurnEndState): "⚙" | "✅" {
  return state.runningJobCount > 0 || state.hasPendingDeliveries || state.hasPendingMessages
    ? "⚙"
    : "✅";
}

export function applyAsyncDetails(runningJobs: Set<string>, details: unknown): void {
  const async = (details as AsyncDetails | undefined)?.async;
  if (!async?.jobId) return;

  if (async.state === "running") {
    runningJobs.add(async.jobId);
  } else if (async.state === "completed" || async.state === "failed") {
    runningJobs.delete(async.jobId);
  }
}
