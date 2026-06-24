# Design: `background-task` (⚙) state for claude-status

**Date:** 2026-06-24
**Status:** Approved — ready for implementation planning
**Repo:** `zjstatus` (ZGEnergy fork)
**Scope:** example scripts + installer only (no Rust, no plugin/wasm change)

## Problem

The per-tab Claude Code status indicator (`{claude_status}`) collapses several
distinct situations into one icon. The specific gap this design closes:

> **When a turn ends but background work (a `run_in_background` task or agent)
> is still running, the tab shows ✅ "done/idle" — indistinguishable from a
> session with nothing left to do.**

The user cannot tell "this session is finished" from "this session ended its
turn but still has background work in flight that will wake it later." The goal
is a distinct **⚙ background-task** state that means *"turn ended, but
backgrounded work is still running."*

This is the genuinely concurrent case: a `run_in_background` task detaches and
outlives the turn, unlike a foreground subagent (which blocks the main loop and
is already covered by the existing ⏳ "thinking" state).

## Non-goals

- **Foreground subagents.** They block the main loop, so the tab is already ⏳
  while they run. No new state needed; `SubagentStart`/`SubagentStop` are NOT
  wired by this design.
- **Showing background work *while actively working*.** In the chosen single-slot
  model, the bg indicator appears at turn boundaries (idle), not during active
  ⏳ work. Making bg visible *during* work would require a stateful composite
  producer or a second placeholder (see Alternatives) — explicitly out of scope.
- **Per-type bg glyphs** (distinguishing `shell` vs `subagent` vs `workflow`
  background tasks). One ⚙ for "something is backgrounded" — YAGNI.
- **Reasoning-vs-tool-execution split** (`MessageDisplay`). Out of scope.

## Mechanism

The authoritative signal is the **`background_tasks[]` array in the `Stop` hook
payload** (Claude Code v2.1.145+; the target environment runs v2.1.190). Per the
hooks reference, this array exists to "distinguish 'session is done' from
'session is paused waiting for background work to wake it back up.'" Each entry
describes one in-flight task with a `type` (`shell`, `subagent`, `monitor`,
`workflow`, `teammate`, `cloud session`, `MCP task`), so reading it covers every
kind of backgrounded work uniformly.

**Core rule — at turn end (`Stop`):**
- `background_tasks` non-empty → emit **⚙**
- `background_tasks` empty or absent → emit **✅**

**Reconcile / lifecycle:** when a background task finishes, it wakes the main
loop; the loop runs briefly and ends, firing `Stop` again. That `Stop` reads a
fresh `background_tasks` snapshot. As long as ≥1 task remains, ⚙ is re-asserted;
when the snapshot is finally empty, it flips to ✅. No dedicated "background
finished" hook exists or is needed — the wake's `Stop` is the reconcile point.

This is a **snapshot** mechanism, not an event counter. That distinction is the
core correctness property (see Edge Cases).

## State set

One glyph at a time (single slot, last-writer-wins — unchanged plugin model):

| State | Glyph | Trigger | Meaning |
|---|---|---|---|
| start | 🤖 | `SessionStart` | session running |
| thinking | ⏳ | `UserPromptSubmit`, `PostToolUse/AskUserQuestion` | main loop working |
| asking | ❓ | `PreToolUse/AskUserQuestion` | blocked on a decision |
| ready | ✅ | `Stop`/`StopFailure` **and** `background_tasks` empty | turn done, nothing backgrounded |
| **background-task** | **⚙** | `Stop`/`StopFailure` **and** `background_tasks` non-empty | **turn done, bg work still running (NEW)** |
| exit | (empty) | `SessionEnd` | clears the icon |

## Architecture

Single-slot, **stateless** producer — preserves the existing design philosophy
(the producer holds no per-pane state; the *plugin* holds state and self-heals).
The plugin already renders whatever string the producer pipes per pane, so a new
state needs **no Rust, no plugin change, no wasm rebuild**. Same release surface
as PR #3: example scripts + installer → a new `claude-status-v1.3` release that
reuses the existing wasm unchanged.

The intelligence moves into the producer's turn-end branch; the installer wiring
barely changes.

## Component changes

### 1. Producer hook — `examples/zjstatus-claude-status.sh`

- **Capture stdin instead of discarding it.** Today the script runs
  `cat > /dev/null` to drain the hook's JSON. The `ready` path now needs that
  JSON, so capture it (e.g. `payload="$(cat)"`).
- **Smarter `ready)` branch.** Decide ⚙ vs ✅ from `background_tasks`:
  - Strip whitespace from the payload and test for the presence of at least one
    array entry: the literal `"background_tasks":[{` after whitespace removal.
    - non-empty → `icon="⚙"`
    - empty (`"background_tasks":[]`) or field absent → `icon="✅"`
  - **Dependency-free** detector (no `jq`, no `python3` at hook runtime). The
    whitespace-strip-then-substring test is robust to pretty-printed JSON and
    degrades safely: an absent field (older Claude Code) yields ✅, matching
    today's behavior.
- **Add ⚙ to the documented states.** Update the header/usage comment; the
  arg vocabulary stays `<start|thinking|asking|ready|exit>` because ⚙ is derived
  *inside* the `ready` path, not a new CLI arg.
- **Exit-0 hardening.** `Stop` and `StopFailure` are decision-control events; a
  nonzero hook exit can interfere with turn completion. The current
  `set -euo pipefail` would propagate a failed `zellij pipe`. Ensure the pipe
  call cannot fail the hook (append `|| true` to the pipe line, or `exit 0` at
  the end). The `set -euo pipefail` discipline is retained elsewhere.

### 2. Installer — `examples/claude-status-setup.sh`

- **Wiring is nearly unchanged.** `Stop` → `ready` already exists; the ⚙/✅
  decision is in the producer, not the wiring.
- **Add one backstop row:** `StopFailure` → `ready` (matcher `None`). If a turn
  ends via API error (`StopFailure` instead of `Stop`), this ensures the icon
  still reconciles. `StopFailure` output/exit code are ignored for decision
  control, but the pipe side-effect still fires.
- The existing matcher-aware idempotency check already handles the new row
  (distinct event key, `matcher=None`), so re-runs stay idempotent.

## Edge cases

1. **Multiple background tasks, staggered completion (the validating case).**
   Launch A, B, C. A finishes, the loop does a minor update, then waits for the
   rest. At every `Stop` boundary the snapshot is read fresh: `[A,B,C]` → ⚙,
   then `[B,C]` → ⚙, then `[C]` → ⚙, then `[]` → ✅. The icon stays ⚙ until the
   *last* task returns. **No false ✅ flicker** when an intermediate task
   finishes — this is precisely the failure mode of an event-counting approach
   (first `SubagentStop` → premature clear) and is the reason the snapshot
   mechanism was chosen over counting `SubagentStart`/`SubagentStop`.

2. **Background-completion wake.** A wake fires no turn-*start* hook (it is not a
   `UserPromptSubmit`), and `PostToolUse`/`MessageDisplay` are not wired, so
   during a wake-driven "minor update" the tab stays ⚙ rather than briefly
   flipping to ⏳. This is accurate (bg work is still pending and dominant) and
   consistent with the single-slot trade-off. Acceptable.

3. **Stale window.** Between a background task finishing and the wake's `Stop`,
   the tab shows a stale ⚙. The wake is near-immediate, so the window is short,
   and a `Stop` always eventually fires — self-healing.

4. **API-error turn end.** Handled by the `StopFailure` backstop row.

5. **Older Claude Code (< v2.1.145).** `background_tasks` absent → detector
   yields ✅ → identical to today's behavior. Graceful degradation; no version
   gating required in the installer.

## Testing

- **Producer (extend the existing sandbox harness, `zellij` stubbed):**
  - `ready` + payload with non-empty `background_tasks` → emits ⚙.
  - `ready` + payload with `"background_tasks":[]` → emits ✅.
  - `ready` + payload with the field absent → emits ✅.
  - Pretty-printed (multi-line) `background_tasks` JSON is detected (whitespace
    robustness).
  - A simulated `zellij pipe` failure still exits 0 (hardening).
  - Existing states (`start`/`thinking`/`asking`/`exit`, no-pane, bogus arg)
    still behave.
- **Installer (extend the merge harness, sandbox settings file):**
  - `StopFailure` → `ready` wired with `matcher=None`.
  - Idempotent re-run (no duplicate rows).
  - Collision-safe with unrelated hooks.
- **End-to-end (live zellij session):** launch a real `run_in_background` agent,
  confirm the tab shows ⚙ after the turn ends, and flips to ✅ once the agent
  completes. Same verification method used for the ❓ asking state.
- `bash -n` on both scripts.

## Alternatives considered

- **Event-counting via `SubagentStart`/`SubagentStop`.** Rejected: counts
  flicker to a false ✅ when an intermediate parallel task finishes, and require
  producer-side state. The `Stop` snapshot is authoritative and stateless.
- **Stateful composite single slot** (producer tracks `{main_state, bg_present}`,
  sends a 2-glyph string so bg is visible *during* active work). Zero Rust but
  reintroduces producer state the project deliberately avoids. Deferred — only
  needed if "bg visible while working" becomes a requirement.
- **Second placeholder `{claude_bg}`** (real two-axis split in the plugin).
  Cleanest separation but costs Rust changes, a wasm rebuild, and a plugin
  release. Disproportionate to the need.

## Release

`claude-status-v1.3`, cut from `main` after merge, reusing the existing wasm
(still zero Rust changes), attaching the updated example files + wasm, marked
latest — same procedure as `claude-status-v1.2`.
