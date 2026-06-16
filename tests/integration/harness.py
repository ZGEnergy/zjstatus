"""Integration-test harness for the zjstatus `{claude_status}` per-tab placeholder.

Adapted from the zellij-claude-status tab-bar plugin's hardened harness. Drives a
REAL headless zellij session (hosted inside tmux as the terminal emulator) and
asserts on the actually-rendered tab bar via `tmux capture-pane`. This exercises
what the rstest unit tests can't: the real `{tabs}` render path, pane->tab
mapping from PaneUpdate, the broadcast `zjstatus::claude_status` pipe, and event
timing — including whether a BACKGROUND tab shows its icon.

zjstatus differs from the other plugin in three ways, all isolated below:
  - layout: zjstatus is configured as the tab bar with `{claude_status}` in the
    tab format strings (see `_gen_layout`);
  - pipe protocol: `zjstatus::claude_status::<pane_id>::<value>` carries the icon
    value directly, keyed by pane id (see `set_status`);
  - state: icons live in the plugin's in-memory `claude_icons` map (no /tmp file),
    kept in sync across per-tab instances by the broadcast pipe.

Requirements: tmux, zellij (0.43+), and the zjstatus wasm built at
target/wasm32-wasip1/release/zjstatus.wasm (run.sh builds it).

Run directly for a smoke check:  python3 harness.py
"""

from __future__ import annotations

import itertools
import os
import shutil
import subprocess
import time

HOME = os.path.expanduser("~")
HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.abspath(os.path.join(HERE, "..", ".."))
# Default to the standard plugins dir so zellij's cached permission grant for
# this path (~/.cache/zellij/permissions.kdl) applies and no y/n prompt blocks
# the headless session. run.sh installs the freshly-built wasm here first.
WASM = os.environ.get("ZJSTATUS_WASM", f"{HOME}/.config/zellij/plugins/zjstatus.wasm")

# Each Session instance appends a UNIQUE suffix (pid + sequence) so no two
# scenarios ever share a zellij session name or pane-id space.
TMUX_SESSION = "zjcs_tmux"
ZJ_SESSION = "zjcs_it"
_SESSION_SEQ = itertools.count(1)


def _unique_suffix():
    return f"{os.getpid()}_{next(_SESSION_SEQ)}"


def _run(args, timeout=10, check=False):
    return subprocess.run(
        args, capture_output=True, text=True, timeout=timeout, check=check
    )


# A distinct truecolor bg for active vs normal tabs, so the harness can detect
# the active tab from the SGR capture. Kept in sync with `_gen_layout`.
ACTIVE_BG = "#5b6cff"
NORMAL_BG = "#1e1e2e"


def _gen_layout(tab_names, workdir, layout):
    """Write a layout with zjstatus as the tab bar (rendering only `{tabs}` with
    `{claude_status}` in each ribbon) and one named tab per name."""
    os.makedirs(workdir, exist_ok=True)
    lines = [
        "layout {",
        "    default_tab_template {",
        "        pane size=1 borderless=true {",
        f'            plugin location="file:{WASM}" {{',
        '                format_left "{tabs}"',
        '                format_center ""',
        '                format_right ""',
        '                format_space ""',
        '                border_enabled "false"',
        f'                tab_normal "#[bg={NORMAL_BG},fg=#cdd6f4] {{name}}{{claude_status}} "',
        f'                tab_active "#[bg={ACTIVE_BG},fg=#ffffff,bold] {{name}}{{claude_status}} "',
        '                tab_separator " "',
        "            }",
        "        }",
        "        children",
        "        pane size=2 borderless=true {",
        '            plugin location="zellij:status-bar"',
        "        }",
        "    }",
    ]
    for i, name in enumerate(tab_names):
        focus = " focus=true" if i == 0 else ""
        lines.append(f'    tab name="{name}"{focus} {{ pane; }}')
    lines.append("}")
    with open(layout, "w") as f:
        f.write("\n".join(lines) + "\n")


# Icon each status event produces on its tab (None == cleared), so set_status can
# verify the inside-pipe injection landed and retry if a keystroke was dropped.
ICON_WORKING = "\U0001f916"
ICON_THINKING = "⏳"
ICON_READY = "✅"
_EVENT_ICON = {
    "start": ICON_WORKING,
    "thinking": ICON_THINKING,
    "ready": ICON_READY,
    "exit": None,
}
_NO_VERIFY = object()


class Bar:
    """A parsed snapshot of the rendered tab bar (row 0 of the capture)."""

    def __init__(self, raw_plain, raw_colored, selected_bg=None):
        self.raw_plain = raw_plain
        self.raw_colored = raw_colored
        self.selected_bg = selected_bg
        self.segments = self._segments(raw_plain)
        self.tabs = [self._split_icon(s) for s in self.segments]
        self.names = [n for n, _ in self.tabs]
        self.name_bg = self._name_bg_map(raw_colored, self.names)
        self.active = self._active()

    @staticmethod
    def _segments(line):
        import re

        parts = re.split(r"\s{2,}", line)
        return [p.strip() for p in parts if p.strip()]

    @staticmethod
    def _split_icon(seg):
        for icon in ("⏳", "\U0001f916", "✅"):  # ⏳ 🤖 ✅
            if seg.endswith(icon):
                return (seg[: -len(icon)].strip(), icon)
        return (seg, None)

    @staticmethod
    def _name_bg_map(colored, names):
        """Map each tab name to the background color (``"r;g;b"`` or ``None``) in
        effect under its first character, by walking the colored capture as an
        SGR state machine."""
        if not colored or not names:
            return {}
        import re

        sgr = re.compile(r"\x1b\[([0-9;]*)m")
        bg = None
        plain = []
        bg_at = []
        i = 0
        while i < len(colored):
            m = sgr.match(colored, i)
            if m:
                parts = m.group(1).split(";")
                j = 0
                while j < len(parts):
                    p = parts[j]
                    if p == "48" and j + 4 < len(parts) and parts[j + 1] == "2":
                        bg = f"{parts[j + 2]};{parts[j + 3]};{parts[j + 4]}"
                        j += 5
                        continue
                    if p in ("49", "0", ""):
                        bg = None
                    j += 1
                i = m.end()
                continue
            plain.append(colored[i])
            bg_at.append(bg)
            i += 1
        text = "".join(plain)
        out = {}
        for name in names:
            idx = text.find(name)
            if idx >= 0:
                out[name] = bg_at[idx]
        return out

    def _active(self):
        if not self.name_bg:
            return None
        if self.selected_bg is not None:
            for name, b in self.name_bg.items():
                if b == self.selected_bg:
                    return name
            return None
        from collections import Counter

        counts = Counter(b for b in self.name_bg.values() if b)
        if not counts:
            return None
        minority = min(counts, key=lambda k: counts[k])
        if counts[minority] != 1:
            return None
        for name, b in self.name_bg.items():
            if b == minority:
                return name
        return None

    def icon_for(self, name):
        for n, icon in self.tabs:
            if n == name:
                return icon
        return None

    def __repr__(self):
        return f"Bar(tabs={self.tabs}, active={self.active!r})"


class Session:
    def __init__(self, tab_names, width=220, height=50):
        self.tab_names = list(tab_names)
        self.width = width
        self.height = height
        self._pane_ids = {}  # tab_index1 -> pane id (str)
        self._selected_bg = None
        suf = _unique_suffix()
        self.zj = f"{ZJ_SESSION}_{suf}"
        self.tmux = f"{TMUX_SESSION}_{suf}"
        self.workdir = f"/tmp/{self.zj}"
        self.layout = f"{self.workdir}/layout.kdl"

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, *exc):
        self.stop()

    def start(self):
        self.stop()  # clean slate
        _gen_layout(self.tab_names, self.workdir, self.layout)
        os.makedirs(self.workdir, exist_ok=True)
        cmd = (
            f"env -u ZELLIJ -u ZELLIJ_SESSION_NAME -u ZELLIJ_PANE_ID "
            f"zellij -s {self.zj} -n {self.layout}"
        )
        _run(
            ["tmux", "new-session", "-d", "-s", self.tmux,
             "-x", str(self.width), "-y", str(self.height), cmd]
        )
        self._approve_permission_if_prompted()
        if not self.wait_for(lambda b: self.tab_names[0] in b.names, timeout=20):
            last = self.bar().raw_plain
            self.stop()
            raise RuntimeError(f"session did not render. last bar: {last!r}")
        prev = -1
        for _ in range(25):
            cur = len(self.bar().names)
            if cur > 0 and cur == prev:
                break
            prev = cur
            time.sleep(0.15)
        time.sleep(0.2)
        # tab 0 starts focused -> its ribbon bg is the theme's "selected" color.
        self._selected_bg = self.bar().name_bg.get(self.tab_names[0])
        return self

    def _approve_permission_if_prompted(self, timeout=8.0):
        deadline = time.time() + timeout
        while time.time() < deadline:
            text = "\n".join(self._cap(False)).lower()
            if "permission" in text and "allow" in text:
                _run(["tmux", "send-keys", "-t", self.tmux, "y"])
                time.sleep(0.6)
                return True
            if self.tab_names[0] in self.bar().names:
                return False
            time.sleep(0.3)
        return False

    def _force_kill(self):
        """Kill anything bound to this session's UNIQUE names (server, client,
        tmux). Safe because the names are unique to this instance."""
        for pat in (self.zj, self.tmux):
            try:
                _run(["pkill", "-9", "-f", pat], timeout=5)
            except subprocess.TimeoutExpired:
                pass

    def stop(self):
        try:
            _run(["zellij", "delete-session", self.zj, "--force"], timeout=8)
        except subprocess.TimeoutExpired:
            self._force_kill()
        try:
            _run(["tmux", "kill-session", "-t", self.tmux], timeout=5)
        except subprocess.TimeoutExpired:
            self._force_kill()
        shutil.rmtree(self.workdir, ignore_errors=True)

    # ---- capture ----------------------------------------------------------

    def _cap(self, colored=False):
        args = ["tmux", "capture-pane", "-t", self.tmux, "-p"]
        if colored:
            args.append("-e")
        r = _run(args)
        return r.stdout.splitlines()

    def bar(self):
        plain = self._cap(False)
        colored = self._cap(True)
        return Bar(
            plain[0] if plain else "",
            colored[0] if colored else "",
            self._selected_bg,
        )

    def wait_for(self, predicate, timeout=8.0, interval=0.3):
        deadline = time.time() + timeout
        last = None
        while time.time() < deadline:
            last = self.bar()
            try:
                if predicate(last):
                    return last
            except Exception:
                pass
            time.sleep(interval)
        return None

    # ---- driving ----------------------------------------------------------

    def _zj(self, *action_args, timeout=6):
        return _run(["zellij", "-s", self.zj, "action", *action_args], timeout=timeout)

    def _run_in_pane(self, cmd):
        """Type `cmd` into the focused pane and run it, confirming the text
        registered on the prompt before pressing Enter (send-keys can silently
        drop chars on a freshly-spawned pane)."""
        _run(["tmux", "send-keys", "-t", self.tmux, "-l", cmd])
        marker = cmd[-24:]
        deadline = time.time() + 2.0
        while time.time() < deadline:
            if marker in "\n".join(self._cap(False)):
                break
            time.sleep(0.1)
        _run(["tmux", "send-keys", "-t", self.tmux, "Enter"])

    def _wait_quiet(self, settle=0.3, timeout=3.0):
        deadline = time.time() + timeout
        prev = None
        while time.time() < deadline:
            cur = "\n".join(self._cap(False))
            if cur == prev:
                return
            prev = cur
            time.sleep(settle)

    def focus_tab(self, index1):
        self._zj("go-to-tab", str(index1))
        time.sleep(0.3)

    def pane_id(self, index1):
        """Discover the pane id of the tab at 1-based position (cached)."""
        if index1 in self._pane_ids:
            return self._pane_ids[index1]
        self.focus_tab(index1)
        pidfile = f"{self.workdir}/pid"
        try:
            os.remove(pidfile)
        except FileNotFoundError:
            pass
        self._run_in_pane(f"echo $ZELLIJ_PANE_ID > {pidfile}")
        deadline = time.time() + 4
        while time.time() < deadline:
            if os.path.exists(pidfile):
                val = open(pidfile).read().strip()
                if val:
                    self._pane_ids[index1] = val
                    return val
            time.sleep(0.2)
        raise RuntimeError(f"could not discover pane id for tab {index1}")

    def set_status(self, index1, event, retries=3):
        """Inject a status onto the tab at 1-based position via the zjstatus pipe
        protocol `zjstatus::claude_status::<pane_id>::<value>`, run INSIDE that
        tab's pane (mirroring the real Claude hook). Verifies the expected icon
        appears (or clears) and retries if a keystroke was dropped on a cold
        shell."""
        self.focus_tab(index1)
        pid = self.pane_id(index1)
        name = self.tab_names[index1 - 1] if 0 <= index1 - 1 < len(self.tab_names) else None
        want = _EVENT_ICON.get(event, _NO_VERIFY)
        value = _EVENT_ICON.get(event, "")
        if value is None:
            value = ""  # clear
        payload = f"zjstatus::claude_status::{pid}::{value}"
        pipe = f'timeout 4 zellij pipe "{payload}" </dev/null'
        for attempt in range(max(1, retries)):
            self._wait_quiet()
            if attempt:
                _run(["tmux", "send-keys", "-t", self.tmux, "C-u"])
                time.sleep(0.2)
            self._run_in_pane(pipe)
            if name is None or want is _NO_VERIFY:
                time.sleep(0.8)
                return
            deadline = time.time() + 2.0
            while time.time() < deadline:
                if self.bar().icon_for(name) == want:
                    time.sleep(0.3)
                    return
                time.sleep(0.2)
        time.sleep(0.3)

    def new_tab(self, name=None):
        self._zj("new-tab")
        time.sleep(0.4)
        if name:
            self.rename_tab(name)

    def rename_tab(self, name):
        self._zj("rename-tab", name)
        time.sleep(0.3)

    def close_tab(self):
        self._zj("close-tab")
        time.sleep(0.4)
        self._pane_ids.clear()

    def move_tab(self, direction):
        r = self._zj("move-tab", direction)
        time.sleep(0.4)
        self._pane_ids.clear()
        return r


if __name__ == "__main__":
    # Smoke check: render three tabs, put a status on the middle one, print bar.
    with Session(["alpha", "beta", "gamma"]) as s:
        b = s.bar()
        print("initial:", b)
        print("plain row0:", repr(s.bar().raw_plain))
        s.set_status(2, "thinking")
        b = s.wait_for(lambda b: b.icon_for("beta") == "⏳", timeout=6) or s.bar()
        print("after thinking on beta:", b)
        print("pane ids:", s._pane_ids)
