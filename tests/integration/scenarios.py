"""Integration scenarios for the zjstatus `{claude_status}` per-tab placeholder.

Each scenario spins up a real zellij session (via harness.Session) with zjstatus
as the tab bar and asserts on the actually-rendered tabs. Run with run.sh (which
builds the wasm first) or directly:  python3 scenarios.py

Exit code is non-zero if any scenario fails.
"""

from __future__ import annotations

import traceback

from harness import Session

WORKING, THINKING, READY = "\U0001f916", "⏳", "✅"


def test_basic_render():
    with Session(["alpha", "beta", "gamma"]) as s:
        b = s.bar()
        assert b.names == ["alpha", "beta", "gamma"], b.names
        assert b.active == "alpha", f"active={b.active!r}"
        assert all(icon is None for _, icon in b.tabs), b.tabs


def test_status_icon_appears():
    with Session(["alpha", "beta", "gamma"]) as s:
        s.set_status(2, "thinking")
        b = s.wait_for(lambda b: b.icon_for("beta") == THINKING)
        assert b, f"icon never appeared: {s.bar()}"
        assert b.names == ["alpha", "beta", "gamma"], b.names


def test_icon_on_background_tab():
    # The whole point: a non-active tab shows its status at a glance.
    with Session(["alpha", "beta", "gamma"]) as s:
        s.set_status(3, "ready")        # gamma busy...
        s.focus_tab(1)                  # ...while we sit on alpha
        b = s.wait_for(lambda b: b.active == "alpha" and b.icon_for("gamma") == READY)
        assert b, f"background-tab icon wrong: {s.bar()}"


def test_background_tab_keeps_icon_after_new_tab():
    # Opening a new tab steals focus; the previously-iconed (now background) tab
    # must keep its icon. This is the PaneUpdate-vs-background-tab question.
    with Session(["one"]) as s:
        s.set_status(1, "start")
        assert s.wait_for(lambda b: b.icon_for("one") == WORKING), s.bar()
        s.new_tab("two")
        assert s.wait_for(lambda b: b.names == ["one", "two"]), s.bar()
        b = s.wait_for(lambda b: b.icon_for("one") == WORKING)
        assert b, f"background tab 'one' lost its icon after opening a new tab: {s.bar()}"


def test_icon_persists_when_other_tab_focused():
    with Session(["one", "two"]) as s:
        s.set_status(1, "start")
        assert s.wait_for(lambda b: b.icon_for("one") == WORKING), s.bar()
        s.focus_tab(2)
        assert s.wait_for(lambda b: b.active == "two"), f"focus did not move: {s.bar()}"
        b = s.wait_for(lambda b: b.icon_for("one") == WORKING)
        assert b, f"'one' lost its icon when 'two' became active: {s.bar()}"


def test_status_transitions():
    with Session(["solo"]) as s:
        for event, icon in [("start", WORKING), ("thinking", THINKING), ("ready", READY)]:
            s.set_status(1, event)
            b = s.wait_for(lambda b, ic=icon: b.icon_for("solo") == ic)
            assert b, f"{event} did not yield {icon}: {s.bar()}"
        s.set_status(1, "exit")
        b = s.wait_for(lambda b: b.icon_for("solo") is None)
        assert b, f"exit did not clear icon: {s.bar()}"


def test_close_middle_tab_keeps_icon():
    # Closing a non-last tab must not move icons to the wrong tab. gamma is busy;
    # close beta; gamma must keep its icon at its new position.
    with Session(["alpha", "beta", "gamma"]) as s:
        s.set_status(3, "ready")
        assert s.wait_for(lambda b: b.icon_for("gamma") == READY), s.bar()
        s.focus_tab(2)          # focus beta
        s.close_tab()           # close beta
        b = s.wait_for(lambda b: b.names == ["alpha", "gamma"])
        assert b, f"tab not closed: {s.bar()}"
        assert b.icon_for("gamma") == READY, f"gamma lost/misplaced its icon: {b}"
        assert b.icon_for("alpha") is None, f"icon bled onto alpha: {b}"


def test_reorder_keeps_icon():
    with Session(["alpha", "beta", "gamma"]) as s:
        s.set_status(1, "start")  # alpha busy (🤖)
        assert s.wait_for(lambda b: b.icon_for("alpha") == WORKING), s.bar()
        s.focus_tab(1)
        r = s.move_tab("right")
        if r.returncode != 0:
            print("    (skipped: `move-tab` action unavailable)")
            return
        b = s.wait_for(lambda b: b.names[:2] == ["beta", "alpha"])
        assert b, f"reorder did not take effect: {s.bar()}"
        assert b.icon_for("alpha") == WORKING, f"icon did not follow alpha: {b}"


SCENARIOS = [
    test_basic_render,
    test_status_icon_appears,
    test_icon_on_background_tab,
    test_background_tab_keeps_icon_after_new_tab,
    test_icon_persists_when_other_tab_focused,
    test_status_transitions,
    test_close_middle_tab_keeps_icon,
    test_reorder_keeps_icon,
]


def main():
    passed, failed = 0, 0
    for fn in SCENARIOS:
        name = fn.__name__
        try:
            fn()
            print(f"PASS  {name}")
            passed += 1
        except Exception as e:
            failed += 1
            print(f"FAIL  {name}: {e}")
            traceback.print_exc()
    print(f"\n{passed} passed, {failed} failed")
    raise SystemExit(1 if failed else 0)


if __name__ == "__main__":
    main()
