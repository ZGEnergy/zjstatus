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
