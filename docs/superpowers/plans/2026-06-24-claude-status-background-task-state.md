# background-task (⚙) claude-status state — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the per-tab Claude status show a distinct ⚙ icon when a turn ends with a `run_in_background` task/agent still running, instead of the ✅ "done/idle" icon.

**Architecture:** Single-slot, stateless producer hook. At turn end (`Stop`/`StopFailure`) the producer reads the hook's JSON stdin and emits ⚙ if the `background_tasks` array is non-empty, else ✅. The plugin already renders whatever per-pane string the producer pipes, so there is **no Rust, plugin, or wasm change** — only the two example scripts plus standalone tests.

**Tech Stack:** Bash (producer hook), Python 3 (installer's embedded settings.json merge), Claude Code hooks, zellij `pipe`.

## Global Constraints

- **No Rust / plugin / wasm changes.** Example scripts + tests only.
- **Stateless producer.** No producer-side per-pane state file; the plugin holds state and self-heals.
- **Dependency-free hook runtime.** No `jq` / `python3` invoked from the producer hook. Detection is a fixed-string substring test.
- **Graceful degradation.** A missing `background_tasks` field (Claude Code < v2.1.145) must yield ✅ — identical to today. No installer version-gating. Target environment runs v2.1.190.
- **Best-effort cosmetic.** The hook must `exit 0` on `Stop`/`StopFailure` even if `zellij pipe` fails (these are decision-control events where a nonzero exit can interfere with turn completion).
- **No layout change.** Single slot — the `{claude_status}` placeholder and `gruvbox-claude-status.kdl` are unchanged.
- **Conventional commits** (`feat:`, `test:`, etc.).
- **Icon vocabulary:** `🤖` start · `⏳` thinking · `❓` asking · `✅` ready (no bg) · `⚙` ready-with-bg · empty = exit.

---

### Task 1: Producer hook — ⚙ background-task detection

**Files:**
- Create: `tests/examples/test_claude_status_hook.sh`
- Modify: `examples/zjstatus-claude-status.sh` (full new contents in Step 3)

**Interfaces:**
- Consumes: nothing (entry-point script).
- Produces: the producer hook `examples/zjstatus-claude-status.sh <start|thinking|asking|ready|exit>`. The `ready` arg reads JSON on stdin and emits `⚙` when stdin (whitespace-stripped) contains the literal substring `"background_tasks":[{`, otherwise `✅`. Emits `zjstatus::claude_status::<ZELLIJ_PANE_ID>::<icon>` via `zellij pipe`. Always exits 0.

- [ ] **Step 1: Write the failing test**

Create `tests/examples/test_claude_status_hook.sh`:

```bash
#!/usr/bin/env bash
# Unit tests for examples/zjstatus-claude-status.sh — per-state icon mapping and
# the `ready` background_tasks detection, with `zellij` stubbed on PATH.
# NOT wired into CI (CI is Rust-only). Run manually:
#   bash tests/examples/test_claude_status_hook.sh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
HOOK="$(cd "$HERE/../.." && pwd)/examples/zjstatus-claude-status.sh"

SHIM="$(mktemp -d)"
make_stub() {  # $1 = exit code the fake `zellij` returns
  cat > "$SHIM/zellij" <<EOF
#!/usr/bin/env bash
echo "PIPE: \$*"
exit $1
EOF
  chmod +x "$SHIM/zellij"
}
make_stub 0
export PATH="$SHIM:$PATH"
export ZELLIJ_PANE_ID="7"

fail=0
run()    { printf '%s' "$2" | "$HOOK" "$1" 2>&1 || true; }
expect() {  # $1 label  $2 actual  $3 needle
  if printf '%s' "$2" | grep -qF "$3"; then
    printf 'ok   %s\n' "$1"
  else
    printf 'FAIL %s\n  got: %s\n  want substring: %s\n' "$1" "$2" "$3"; fail=1
  fi
}

expect "start -> robot"         "$(run start    '{}')" "::7::🤖"
expect "thinking -> hourglass"  "$(run thinking '{}')" "::7::⏳"
expect "asking -> question"     "$(run asking   '{}')" "::7::❓"
expect "exit -> empty value"    "$(run exit     '{}')" "::7::"

expect "ready empty array -> check"  "$(run ready '{"background_tasks":[]}')"      "::7::✅"
expect "ready absent field -> check" "$(run ready '{"hook_event_name":"Stop"}')"   "::7::✅"

BG='{"hook_event_name":"Stop","background_tasks":[{"id":"t1","type":"subagent","status":"running"}]}'
expect "ready non-empty -> gear"     "$(run ready "$BG")" "::7::⚙"

PRETTY=$'{\n  "background_tasks": [\n    { "id": "t1" }\n  ]\n}'
expect "ready pretty-printed -> gear" "$(run ready "$PRETTY")" "::7::⚙"

# Hardening: a failing `zellij pipe` must still exit 0 (Stop is decision-control).
make_stub 1
if printf '%s' '{}' | "$HOOK" ready >/dev/null 2>&1; then
  printf 'ok   ready exits 0 even when zellij pipe fails\n'
else
  printf 'FAIL ready exits non-zero when zellij pipe fails\n'; fail=1
fi

rm -rf "$SHIM"
exit "$fail"
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `bash tests/examples/test_claude_status_hook.sh`
Expected: FAIL — the current `ready` branch always emits `✅` (so both gear tests fail), and the current hook lacks `|| true` so the pipe-failure case exits non-zero (hardening test fails).

- [ ] **Step 3: Implement — replace the producer hook contents**

Overwrite `examples/zjstatus-claude-status.sh` with:

```bash
#!/usr/bin/env bash
# Sends Claude Code status to zjstatus's {claude_status} placeholder.
#
# Usage: zjstatus-claude-status.sh <start|thinking|asking|ready|exit>
#   start    -> 🤖 (session running)
#   thinking -> ⏳ (working: prompt submitted / resumed after a question)
#   asking   -> ❓ (blocking on an AskUserQuestion — needs your answer)
#   ready    -> ✅ (turn done, awaiting input)
#               ⚙ (turn done, a run_in_background task/agent is still running)
#   exit     -> (empty, clears the tab's icon)
#
# `ready` is wired to Stop/StopFailure. It inspects the hook's JSON stdin: a
# non-empty `background_tasks` array means backgrounded work outlived the turn,
# so it renders ⚙ instead of ✅. Detection is a dependency-free fixed-string
# test (no jq/python); an absent field (older Claude Code) falls through to ✅.
set -euo pipefail

# Not inside a Zellij pane -> nothing to address.
[ -z "${ZELLIJ_PANE_ID:-}" ] && exit 0
payload="$(cat)"   # the hook's JSON stdin (consumed by the `ready` path)

case "${1:-}" in
  start)    icon="🤖" ;;
  thinking) icon="⏳" ;;
  asking)   icon="❓" ;;
  ready)
    # ⚙ if a background task/agent is still running, else ✅. Strip whitespace so
    # pretty-printed JSON is matched, then look for the literal start of a
    # non-empty array. -F = fixed string, so no regex escaping of []{}.
    if printf '%s' "$payload" | tr -d '[:space:]' | grep -qF '"background_tasks":[{'; then
      icon="⚙"
    else
      icon="✅"
    fi
    ;;
  exit)     icon="" ;;     # empty value clears the icon for this pane
  *)        exit 0 ;;
esac

# Best-effort cosmetic: never fail the hook. Stop/StopFailure are decision-control
# events where a nonzero exit can interfere with turn completion.
zellij pipe --name zjstatus -- "zjstatus::claude_status::${ZELLIJ_PANE_ID}::${icon}" || true
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `bash tests/examples/test_claude_status_hook.sh`
Expected: every line prints `ok   ...`; exit code 0.

- [ ] **Step 5: Syntax-check the hook**

Run: `bash -n examples/zjstatus-claude-status.sh`
Expected: no output, exit 0.

- [ ] **Step 6: Commit**

```bash
git add tests/examples/test_claude_status_hook.sh examples/zjstatus-claude-status.sh
git commit -m "feat(examples): add ⚙ background-task state to claude-status hook"
```

---

### Task 2: Installer — `StopFailure` backstop wiring

**Files:**
- Create: `tests/examples/test_claude_status_setup.py`
- Modify: `examples/claude-status-setup.sh` (the `wire` list inside the embedded `python3 - <<'PY'` block)

**Interfaces:**
- Consumes: the producer hook from Task 1 (the installer wires its path into `~/.claude/settings.json`).
- Produces: `settings.json` with seven wired events — `SessionStart`, `UserPromptSubmit`, `PreToolUse`(AskUserQuestion), `PostToolUse`(AskUserQuestion), `Stop`, `StopFailure`, `SessionEnd`. `Stop` and `StopFailure` both map to `ready` with no matcher. Re-running the installer stays idempotent.

- [ ] **Step 1: Write the failing test**

Create `tests/examples/test_claude_status_setup.py`:

```python
#!/usr/bin/env python3
"""Unit test for the settings.json merge block embedded in
examples/claude-status-setup.sh. Extracts the real Python heredoc and runs it
against a sandbox settings file (never touches ~/.claude). NOT wired into CI
(CI is Rust-only). Run manually:
    python3 tests/examples/test_claude_status_setup.py
"""
import json, os, re, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "..", "..", "examples", "claude-status-setup.sh")
src = open(SRC).read()

m = re.search(r"<<'PY'\n(.*?)\nPY\n", src, re.DOTALL)
assert m, "could not find embedded PY block in installer"
block = m.group(1)

tmp = tempfile.mkdtemp()
settings = os.path.join(tmp, "settings.json")

def run():
    return subprocess.run([sys.executable, "-c", block],
                          env={**os.environ, "SETTINGS_PATH": settings},
                          capture_output=True, text=True)

fail = 0
def check(label, cond):
    global fail
    print(("ok   " if cond else "FAIL ") + label)
    if not cond:
        fail = 1

r = run()
check("merge runs cleanly", r.returncode == 0)
hooks = json.load(open(settings)).get("hooks", {})

sf = hooks.get("StopFailure", [])
check("StopFailure -> ready wired, matcher=None",
      len(sf) == 1
      and sf[0].get("matcher") is None
      and sf[0]["hooks"][0]["command"].endswith("ready"))

st = hooks.get("Stop", [])
check("Stop -> ready still wired",
      len(st) == 1 and st[0]["hooks"][0]["command"].endswith("ready"))

total1 = sum(len(v) for v in hooks.values())
check("seven events wired on fresh install", total1 == 7)

run()  # idempotency
total2 = sum(len(v) for v in json.load(open(settings))["hooks"].values())
check("idempotent re-run (no duplicates)", total1 == total2 == 7)

sys.exit(fail)
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `python3 tests/examples/test_claude_status_setup.py`
Expected: FAIL — the installer currently wires six events with no `StopFailure`, so the `StopFailure` check fails and the count is 6, not 7.

- [ ] **Step 3: Implement — add the `StopFailure` row**

In `examples/claude-status-setup.sh`, find the `wire` list:

```python
wire = [
    ("SessionStart",     "start",    None),
    ("UserPromptSubmit", "thinking", None),
    ("PreToolUse",       "asking",   "AskUserQuestion"),
    ("PostToolUse",      "thinking", "AskUserQuestion"),
    ("Stop",             "ready",    None),
    ("SessionEnd",       "exit",     None),
]
```

Replace it with (adds the `StopFailure` backstop after `Stop`):

```python
wire = [
    ("SessionStart",     "start",    None),
    ("UserPromptSubmit", "thinking", None),
    ("PreToolUse",       "asking",   "AskUserQuestion"),
    ("PostToolUse",      "thinking", "AskUserQuestion"),
    ("Stop",             "ready",    None),
    # StopFailure: turn ended via API error. Reconcile the icon (✅/⚙) too, so a
    # background task outliving an errored turn is still reflected.
    ("StopFailure",      "ready",    None),
    ("SessionEnd",       "exit",     None),
]
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `python3 tests/examples/test_claude_status_setup.py`
Expected: every line prints `ok   ...`; exit code 0.

- [ ] **Step 5: Syntax-check the installer (shell + embedded Python)**

Run: `bash -n examples/claude-status-setup.sh`
Expected: no output, exit 0.

Run: `python3 - <<'EOF'`
```python
import ast, re
src = open("examples/claude-status-setup.sh").read()
block = re.search(r"<<'PY'\n(.*?)\nPY\n", src, re.DOTALL).group(1)
ast.parse(block)
print("embedded python parses clean")
EOF
```
Expected: `embedded python parses clean`.

- [ ] **Step 6: Commit**

```bash
git add tests/examples/test_claude_status_setup.py examples/claude-status-setup.sh
git commit -m "feat(examples): wire StopFailure backstop in claude-status installer"
```

---

### Task 3: Live end-to-end verification + release

This task is manual verification (no automated harness can launch a real backgrounded agent) followed by the release, mirroring the `claude-status-v1.2` procedure. Do this only after Tasks 1–2 are merged to `main`.

**Files:** none modified. Uses the merged `examples/*` and the existing wasm.

**Interfaces:**
- Consumes: the merged producer hook and installer.
- Produces: a `claude-status-v1.3` GitHub release marked latest, with assets `zjstatus.wasm`, `claude-status-setup.sh`, `gruvbox-claude-status.kdl`, `zjstatus-claude-status.sh`.

- [ ] **Step 1: Install the merged scripts locally**

Run: `bash examples/claude-status-setup.sh`
Expected: `==> Wiring hooks ...` and `added: ... StopFailure ...` (or "already wired" if re-run). Confirms the installer merges the new row into `~/.claude/settings.json`.

- [ ] **Step 2: Verify live in a zellij session**

In a zellij pane running Claude Code, dispatch a real background agent (e.g. an `Agent` call with `run_in_background: true`, or any `run_in_background` Bash that sleeps ~60s). Observe this tab:
- While the turn is active: ⏳.
- After the turn ends with the background task still running: **⚙**.
- After the background task completes and the next turn settles: **✅**.

Expected: the tab shows ⚙ during the post-turn background window and reconciles to ✅ once the task finishes. (Same observation method used to verify the ❓ asking state.)

- [ ] **Step 3: Confirm no Rust delta since the last release (wasm reuse is safe)**

Run: `git diff --stat claude-status-v1.2 HEAD -- src/ Cargo.toml Cargo.lock`
Expected: empty output (no Rust/dependency changes), so the existing wasm can be reused.

- [ ] **Step 4: Download the current release wasm to reuse**

Run: `gh release download claude-status-v1.2 --pattern zjstatus.wasm --dir /tmp/zjs-v13 --clobber`
Expected: `zjstatus.wasm` saved under `/tmp/zjs-v13/`.

- [ ] **Step 5: Cut the release**

Run:
```bash
gh release create claude-status-v1.3 \
  --target main \
  --title "zjstatus + per-tab Claude status (v1.3)" \
  --notes "Adds a ⚙ background-task state: when a turn ends with a run_in_background task or agent still running, the tab shows ⚙ instead of ✅, reconciling to ✅ once the work finishes. No Rust changes — zjstatus.wasm is identical to v1.2." \
  /tmp/zjs-v13/zjstatus.wasm \
  examples/claude-status-setup.sh \
  examples/gruvbox-claude-status.kdl \
  examples/zjstatus-claude-status.sh
```
Expected: prints the release URL.

- [ ] **Step 6: Verify the release is latest with all four assets**

Run: `gh release view --json tagName,assets -q '{tag:.tagName, assets:[.assets[].name]}'`
Expected: `tag` = `claude-status-v1.3`; assets list contains `zjstatus.wasm`, `claude-status-setup.sh`, `gruvbox-claude-status.kdl`, `zjstatus-claude-status.sh`.

---

## Self-Review

**Spec coverage:**
- ⚙ at turn end when `background_tasks` non-empty → Task 1 (Steps 1, 3).
- ✅ when empty/absent (graceful degradation) → Task 1 (empty + absent test cases).
- Dependency-free detector, whitespace-robust → Task 1 (`tr -d` + `grep -qF`; pretty-printed test case).
- Exit-0 hardening → Task 1 (`|| true`; pipe-failure test).
- `StopFailure` backstop → Task 2.
- Idempotent installer merge → Task 2 (idempotency check).
- No Rust / layout change → Global Constraints; verified in Task 3 Step 3.
- Multi-task lifecycle (snapshot, no false ✅ flicker) → property of the `background_tasks` snapshot mechanism; observed in Task 3 Step 2. No code beyond Task 1's `ready` branch is needed (snapshot is read fresh at each `Stop`).
- Release v1.3 reusing wasm → Task 3.

**Placeholder scan:** none — every code/test step shows complete content; commands have expected output.

**Type/name consistency:** the `ready` arg, the `"background_tasks":[{` fixed-string needle, the seven wired events, and the `claude-status-v1.3` tag are used identically across tasks and tests.

**No-test-for-examples note:** the repo had no shell test harness for `examples/`; this plan adds standalone runnable tests under `tests/examples/`. They are intentionally not wired into CI (CI is Rust-only). Wiring a shell-test CI job is a possible follow-up, out of scope here.
