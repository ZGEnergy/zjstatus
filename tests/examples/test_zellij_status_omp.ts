import { strict as assert } from "node:assert";

import {
  applyAsyncDetails,
  iconForAskPhase,
  iconForSessionEvent,
  iconForTurnEnd,
} from "../../examples/omp-status-logic.mts";

assert.equal(iconForSessionEvent("start"), "🤖");
assert.equal(iconForSessionEvent("thinking"), "⏳");
assert.equal(iconForSessionEvent("shutdown"), "");

assert.equal(iconForAskPhase("call"), "❓");
assert.equal(iconForAskPhase("result"), "⏳");

assert.equal(
  iconForTurnEnd({ runningJobCount: 0, hasPendingDeliveries: false, hasPendingMessages: false }),
  "✅",
);
assert.equal(
  iconForTurnEnd({ runningJobCount: 1, hasPendingDeliveries: false, hasPendingMessages: false }),
  "⚙",
);
assert.equal(
  iconForTurnEnd({ runningJobCount: 0, hasPendingDeliveries: true, hasPendingMessages: false }),
  "⚙",
);
assert.equal(
  iconForTurnEnd({ runningJobCount: 0, hasPendingDeliveries: false, hasPendingMessages: true }),
  "⚙",
);

const runningJobs = new Set<string>();
applyAsyncDetails(runningJobs, { async: { state: "running", jobId: "bash-1", type: "bash" } });
assert.deepEqual([...runningJobs], ["bash-1"]);
applyAsyncDetails(runningJobs, { async: { state: "completed", jobId: "bash-1", type: "bash" } });
assert.equal(runningJobs.size, 0);
applyAsyncDetails(runningJobs, { async: { state: "running", jobId: "task-1", type: "task" } });
applyAsyncDetails(runningJobs, { async: { state: "failed", jobId: "task-1", type: "task" } });
assert.equal(runningJobs.size, 0);

console.log("OMP status logic tests passed");
