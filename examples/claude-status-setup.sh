#!/usr/bin/env bash
# One-shot installer for the ZGEnergy zjstatus fork + per-tab status icons.
# Idempotent and safe to re-run. Installs the plugin and the Gruvbox layout.
# For Claude Code: installs the producer hook and wires events into ~/.claude/settings.json.
# For omp: installs the extension into ~/.omp/agent/extensions/ (auto-discovered).
#
# Usage:
#   curl -fsSL https://github.com/ZGEnergy/zjstatus/releases/latest/download/claude-status-setup.sh | bash
#   # with keybinding hints:
#   curl -fsSL .../claude-status-setup.sh | bash -s -- --with-hints
#   # with omp extension instead of (or alongside) Claude Code hooks:
#   curl -fsSL .../claude-status-setup.sh | bash -s -- --omp
#   curl -fsSL .../claude-status-setup.sh | bash -s -- --omp --with-hints
#
# After it finishes, open a NEW zellij session and approve the plugin permission.
set -euo pipefail

REL="https://github.com/ZGEnergy/zjstatus/releases/latest/download"
HINTS_REL="https://github.com/b0o/zjstatus-hints/releases/latest/download"
PLUGINS="$HOME/.config/zellij/plugins"
LAYOUTS="$HOME/.config/zellij/layouts"
HOOKS_DIR="$HOME/.claude/hooks"
SETTINGS="$HOME/.claude/settings.json"
OMP_EXT_DIR="$HOME/.omp/agent/extensions"
WITH_HINTS=0
WITH_OMP=0
for arg in "$@"; do
  case "$arg" in
    --with-hints) WITH_HINTS=1 ;;
    --omp)        WITH_OMP=1 ;;
  esac
done

say()  { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[!]\033[0m %s\n' "$*"; }
need() { command -v "$1" >/dev/null 2>&1 || { warn "missing required tool '$1'"; exit 1; }; }
need curl
need python3

# 1. plugin wasm
say "Installing zjstatus.wasm -> $PLUGINS"
mkdir -p "$PLUGINS"
curl -fsSL "$REL/zjstatus.wasm" -o "$PLUGINS/zjstatus.wasm"

# 2. Gruvbox layout (back up a pre-existing non-ours default.kdl, once)
say "Installing Gruvbox layout -> $LAYOUTS/default.kdl"
mkdir -p "$LAYOUTS"
DEF="$LAYOUTS/default.kdl"
if [ -f "$DEF" ] && ! grep -q '{claude_status}' "$DEF" 2>/dev/null; then
  BAK="$DEF.pre-claude-status.bak"
  if [ ! -f "$BAK" ]; then cp "$DEF" "$BAK"; warn "backed up your existing default.kdl -> $BAK"; fi
fi
curl -fsSL "$REL/gruvbox-claude-status.kdl" -o "$DEF"

# 3. Claude Code producer hook
say "Installing hook -> $HOOKS_DIR/zjstatus-claude-status.sh"
mkdir -p "$HOOKS_DIR"
curl -fsSL "$REL/zjstatus-claude-status.sh" -o "$HOOKS_DIR/zjstatus-claude-status.sh"
chmod +x "$HOOKS_DIR/zjstatus-claude-status.sh"

# 4. wire the hook events into settings.json (idempotent JSON merge, keeps your other hooks)
say "Wiring hooks into $SETTINGS"
mkdir -p "$(dirname "$SETTINGS")"
if [ -f "$SETTINGS" ]; then cp "$SETTINGS" "$SETTINGS.bak.$(date +%Y%m%d%H%M%S)"; fi
SETTINGS_PATH="$SETTINGS" python3 - <<'PY'
import json, os
p = os.environ["SETTINGS_PATH"]
try:
    with open(p) as f:
        data = json.load(f)
except (FileNotFoundError, json.JSONDecodeError):
    data = {}
if not isinstance(data, dict):
    data = {}
hooks = data.setdefault("hooks", {})
if not isinstance(hooks, dict):
    hooks = data["hooks"] = {}
cmd = "$HOME/.claude/hooks/zjstatus-claude-status.sh"
# (event, arg, matcher) — matcher is None for whole-session events, or a tool
# name for tool-scoped events. AskUserQuestion is a tool, so it is wired via
# PreToolUse/PostToolUse (NOT the Notification hook, which is an overloaded
# ~60s idle nudge that collides with background-standby).
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
added = []
for event, arg, matcher in wire:
    arr = hooks.get(event)
    if not isinstance(arr, list):
        arr = hooks[event] = []
    # idempotent: skip if our hook with this same matcher is already present
    present = any(
        isinstance(h, dict) and "zjstatus-claude-status.sh" in str(h.get("command", ""))
        and entry.get("matcher") == matcher
        for entry in arr if isinstance(entry, dict)
        for h in entry.get("hooks", []) if isinstance(h, dict)
    )
    if not present:
        entry = {"hooks": [{"type": "command", "command": f"{cmd} {arg}"}]}
        if matcher:
            entry["matcher"] = matcher
        arr.append(entry)
        added.append(event)
with open(p, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
print("  added:" , ", ".join(added) if added else "(nothing — already wired)")
PY

# 5. omp extension (optional, --omp)
if [ "$WITH_OMP" = 1 ]; then
  say "Installing omp extension -> $OMP_EXT_DIR/zellij-status.ts"
  mkdir -p "$OMP_EXT_DIR"
  curl -fsSL "$REL/zjstatus-claude-status-omp.ts" -o "$OMP_EXT_DIR/zellij-status.ts"
  cat <<'EOS'

  [omp] The extension is auto-discovered from ~/.omp/agent/extensions/. No
  settings wiring needed — omp loads it on the next session start.

  Events mapped (omp → icon):
    session_start     🤖    turn_end (idle)      ✅
    turn_start        ⏳     turn_end (pending)    ⚙
    tool_call(ask)    ❓    session_shutdown      (clear)
    tool_result(ask)  ⏳
EOS
fi

# 6. optional keybinding hints
if [ "$WITH_HINTS" = 1 ]; then
  say "Installing zjstatus-hints.wasm"
  curl -fsSL "$HINTS_REL/zjstatus-hints.wasm" -o "$PLUGINS/zjstatus-hints.wasm"
  cat <<'EOS'

  [hints] config.kdl is KDL (not auto-edited). Add to the plugins { } block in
  ~/.config/zellij/config.kdl:

      zjstatus-hints location="file:~/.config/zellij/plugins/zjstatus-hints.wasm" {
          pipe_name "zjstatus_hints"
          hide_in_base_mode false
      }

  …and add (or merge into) a top-level:

      load_plugins {
          zjstatus-hints
      }
EOS
fi

# 7. detect the old rename-API plugin and warn (does not auto-remove)
OLD=0
for f in "$HOME/.config/zellij/config.kdl" "$SETTINGS" "$LAYOUTS"/*.kdl; do
  if [ -f "$f" ] && grep -q 'zellij-claude-status' "$f" 2>/dev/null; then
    warn "old rename-API plugin still referenced in: $f"; OLD=1
  fi
done
if [ "$OLD" = 1 ]; then
  cat <<'EOS'

  [cleanup] To disable the OLD claude-status plugin (the buggy rename-API one):
    - config.kdl : drop zellij-claude-status from load_plugins { } and plugins { }
    - layout(s)  : remove any zellij-claude-status plugin pane
    - settings.json: delete the old hook entries that ran it
    - optional   : rm ~/.config/zellij/plugins/zellij-claude-status.wasm
EOS
fi

say "Done. Open a NEW zellij session and approve the plugin permission prompt."
if [ "$WITH_OMP" = 1 ]; then
  say "omp extension installed — start a new omp session to activate."
else
  say "Tweak datetime_timezone in $DEF to your zone."
fi
