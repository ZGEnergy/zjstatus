#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin" "$TMP/home"

cat >"$TMP/bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
out=""
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o)
      out="$2"
      shift 2
      ;;
    *)
      url="$1"
      shift
      ;;
  esac
done
[ -n "$out" ]
[ -n "$url" ]
printf '%s\n' "$url" >>"$CURL_LOG"
mkdir -p "$(dirname "$out")"
printf 'fake release asset\n' >"$out"
EOF
chmod +x "$TMP/bin/curl"

CURL_LOG="$TMP/curl.log" HOME="$TMP/home" PATH="$TMP/bin:$PATH" \
  bash "$ROOT/examples/claude-status-setup.sh" --omp >"$TMP/output"

extension="$TMP/home/.omp/agent/extensions/zellij-status.ts"
logic="$TMP/home/.omp/agent/extensions/omp-status-logic.mts"
[ -f "$extension" ]
[ -f "$logic" ]
grep -Fqx 'https://github.com/ZGEnergy/zjstatus/releases/latest/download/zjstatus-claude-status-omp.ts' "$TMP/curl.log"
grep -Fqx 'https://github.com/ZGEnergy/zjstatus/releases/latest/download/omp-status-logic.mts' "$TMP/curl.log"
grep -Fq 'omp extension installed' "$TMP/output"

echo "OMP installer test passed"
