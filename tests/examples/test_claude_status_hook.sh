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
