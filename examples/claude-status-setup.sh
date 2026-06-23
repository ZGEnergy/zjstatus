#!/usr/bin/env bash
# One-shot installer for the ZGEnergy zjstatus fork + per-tab Claude Code status.
# Idempotent and safe to re-run. Installs the plugin, the Gruvbox layout, and the
# Claude Code hook, and merges the 4 hook events into ~/.claude/settings.json.
#
# Usage:
#   curl -fsSL https://github.com/ZGEnergy/zjstatus/releases/latest/download/claude-status-setup.sh | bash
#   # with keybinding hints:
#   curl -fsSL .../claude-status-setup.sh | bash -s -- --with-hints
#
# After it finishes, open a NEW zellij session and approve the plugin permission.
set -euo pipefail

REL="https://github.com/ZGEnergy/zjstatus/releases/latest/download"
HINTS_REL="https://github.com/b0o/zjstatus-hints/releases/latest/download"
PLUGINS="$HOME/.config/zellij/plugins"
LAYOUTS="$HOME/.config/zellij/layouts"
HOOKS_DIR="$HOME/.claude/hooks"
SETTINGS="$HOME/.claude/settings.json"
WITH_HINTS=0
[ "${1:-}" = "--with-hints" ] && WITH_HINTS=1

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

# 4. wire the 4 hook events into settings.json (idempotent JSON merge, keeps your other hooks)
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
wire = {"SessionStart": "start", "UserPromptSubmit": "thinking", "Stop": "ready", "SessionEnd": "exit"}
added = []
for event, arg in wire.items():
    arr = hooks.get(event)
    if not isinstance(arr, list):
        arr = hooks[event] = []
    present = any(
        isinstance(h, dict) and "zjstatus-claude-status.sh" in str(h.get("command", ""))
        for entry in arr if isinstance(entry, dict)
        for h in entry.get("hooks", []) if isinstance(h, dict)
    )
    if not present:
        arr.append({"hooks": [{"type": "command", "command": f"{cmd} {arg}"}]})
        added.append(event)
with open(p, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
print("  added:" , ", ".join(added) if added else "(nothing — already wired)")
PY

# 5. optional keybinding hints
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

# 6. detect the old rename-API plugin and warn (does not auto-remove)
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
say "Tweak datetime_timezone in $DEF to your zone."
