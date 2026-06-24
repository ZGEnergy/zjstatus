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
