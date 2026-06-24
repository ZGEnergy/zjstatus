#!/usr/bin/env bash
# Sends Claude Code status to zjstatus's {claude_status} placeholder.
#
# Usage: zjstatus-claude-status.sh <start|thinking|asking|ready|exit>
#   start    -> 🤖 (session running)
#   thinking -> ⏳ (working: prompt submitted / resumed after a question)
#   asking   -> ❓ (blocking on an AskUserQuestion — needs your answer)
#   ready    -> ✅ (stopped, awaiting input)
#   exit     -> (empty, clears the tab's icon)
set -euo pipefail

# Not inside a Zellij pane -> nothing to address.
[ -z "${ZELLIJ_PANE_ID:-}" ] && exit 0
cat > /dev/null   # drain the hook's JSON stdin

case "${1:-}" in
  start)    icon="🤖" ;;
  thinking) icon="⏳" ;;
  asking)   icon="❓" ;;
  ready)    icon="✅" ;;
  exit)     icon="" ;;     # empty value clears the icon for this pane
  *)        exit 0 ;;
esac

zellij pipe --name zjstatus -- "zjstatus::claude_status::${ZELLIJ_PANE_ID}::${icon}"
