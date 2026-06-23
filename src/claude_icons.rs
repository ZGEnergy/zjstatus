//! Cross-instance sharing for the `{claude_status}` per-tab icons.
//!
//! zjstatus is loaded once per tab (via `default_tab_template`), so a
//! `claude_status` pipe only reaches the instances that exist at that moment —
//! a tab opened later starts with an empty icon map. To converge, every instance
//! mirrors its per-pane icon map to a per-session file and reloads it on the
//! events all instances receive (PaneUpdate/TabUpdate/Timer). Writes are
//! read-merge-write so concurrent instances don't clobber each other's panes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use uuid::Uuid;

const DIR: &str = "/tmp/zjstatus";

/// Filesystem-safe icon-file path for a session (non `[A-Za-z0-9_-]` -> `_`).
pub fn icon_file(session: &str) -> PathBuf {
    let safe: String = session
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    PathBuf::from(format!("{DIR}/{safe}.icons"))
}

/// Serialize the per-pane icon map to one `id=value` line per entry. Values
/// never contain newlines (the pipe protocol forbids them).
pub fn serialize(icons: &BTreeMap<u32, String>) -> String {
    let mut out = String::new();
    for (id, value) in icons {
        out.push_str(&format!("{id}={value}\n"));
    }
    out
}

/// Parse the text form back into a map, skipping malformed/empty lines.
pub fn parse(text: &str) -> BTreeMap<u32, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let Some((id, value)) = line.split_once('=') else {
            continue;
        };
        let Ok(id) = id.parse::<u32>() else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        map.insert(id, value.to_owned());
    }
    map
}

/// Atomically replace the session file's contents with `map`. Writes to a unique
/// temp file, then renames it over the target.
///
/// A plain `fs::write` truncates the file before writing it, so a concurrent
/// instance (one zjstatus runs per tab, all sharing this file) can read a
/// half-written file and persist it back, permanently dropping another tab's
/// icon. The rename is atomic within the directory, so every reader observes a
/// complete file — either the old contents or the new, never a partial one. All
/// errors are ignored — the icon is best-effort cosmetic state.
fn write_atomic(path: &Path, map: &BTreeMap<u32, String>) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("icons");
    let tmp = path.with_file_name(format!("{file_name}.{}.tmp", Uuid::new_v4()));
    if std::fs::write(&tmp, serialize(map)).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Apply a single pane's status change to the shared session file. Read-merge-
/// write so a concurrent instance's panes are preserved; an empty value clears
/// the pane. All errors are ignored — the icon is best-effort cosmetic state.
///
/// Residual race (intentionally not locked): the read-merge-write is *not*
/// guarded by a file lock — flock isn't reliably available under the WASI
/// sandbox — so two per-tab instances writing DIFFERENT panes at the very same
/// moment can each read the pre-update file, merge in only their own pane, and
/// race to rename; the later rename wins and silently drops the other's update.
/// This is acceptable because the state is best-effort and self-healing: each
/// producer re-sends its pane's status on the next change (so a dropped update
/// reappears), and stale entries left by dead panes are pruned by [`prune`]. A
/// sequential (non-racing) distinct-pane merge — the common case — is preserved
/// correctly, which is what `persist_sequential_distinct_panes_both_survive`
/// exercises.
pub fn persist(session: &str, pane_id: u32, value: &str) {
    let path = icon_file(session);
    let mut map = match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(_) => BTreeMap::new(),
    };
    if value.is_empty() {
        map.remove(&pane_id);
    } else {
        map.insert(pane_id, value.to_owned());
    }
    write_atomic(&path, &map);
}

/// Drop every entry whose pane id is not in `live_ids`, then write the pruned map
/// back to the session file. Returns the pruned map.
///
/// An entry normally leaves the file only when its producer sends an explicit
/// empty-value clear (the `exit` hook). A Claude pane that dies without firing
/// that hook (pane killed, terminal closed) would otherwise leak its `id=value`
/// line forever — showing a stale icon when the pane id is later reused, and
/// letting a restarted same-named session inherit old icons. Callers pass the
/// set of live pane ids derived from the current `PaneManifest` (terminal panes,
/// the same id space `pick_claude_status` reads). The file is only rewritten when
/// something was actually removed, and (like [`persist`]) all errors are ignored.
pub fn prune(session: &str, live_ids: &BTreeSet<u32>) -> BTreeMap<u32, String> {
    let path = icon_file(session);
    let mut map = match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(_) => return BTreeMap::new(),
    };
    let before = map.len();
    map.retain(|id, _| live_ids.contains(id));
    if map.len() != before {
        write_atomic(&path, &map);
    }
    map
}

/// Load the shared session icon map (the union across all instances). Returns an
/// empty map if the file is absent or unreadable.
pub fn reload(session: &str) -> BTreeMap<u32, String> {
    match std::fs::read_to_string(icon_file(session)) {
        Ok(text) => parse(&text),
        Err(_) => BTreeMap::new(),
    }
}

#[cfg(test)]
mod test {
    use super::{icon_file, parse, persist, prune, reload, serialize};
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn serialize_parse_roundtrip() {
        let mut icons = BTreeMap::new();
        icons.insert(1u32, "🤖".to_owned());
        icons.insert(42u32, "✅".to_owned());

        let back = parse(&serialize(&icons));

        assert_eq!(back, icons);
    }

    #[test]
    fn parse_skips_malformed_lines() {
        let text = "1=🤖\nnonsense\n=⏳\nabc=✅\n7=\n9=ok\n";

        let map = parse(text);

        // kept: 1=🤖 and 9=ok. dropped: no '=', empty id, non-numeric id, empty value.
        assert_eq!(map.get(&1), Some(&"🤖".to_owned()));
        assert_eq!(map.get(&9), Some(&"ok".to_owned()));
        assert_eq!(map.len(), 2, "{map:?}");
    }

    #[test]
    fn icon_file_sanitizes_session_name() {
        let p = icon_file("zjcs/weird name.1");
        assert_eq!(p.to_string_lossy(), "/tmp/zjstatus/zjcs_weird_name_1.icons");
    }

    #[test]
    fn persist_then_reload_roundtrips_through_a_file() {
        let session = "zjtest_persist_reload";
        let _ = std::fs::remove_file(icon_file(session));

        persist(session, 5, "🤖");
        persist(session, 6, "✅");

        let map = reload(session);
        assert_eq!(map.get(&5), Some(&"🤖".to_owned()));
        assert_eq!(map.get(&6), Some(&"✅".to_owned()));

        let _ = std::fs::remove_file(icon_file(session));
    }

    #[test]
    fn persist_empty_value_clears_one_pane_keeps_others() {
        let session = "zjtest_persist_clear";
        let _ = std::fs::remove_file(icon_file(session));

        persist(session, 5, "🤖");
        persist(session, 6, "✅");
        persist(session, 5, ""); // clear only pane 5

        let map = reload(session);
        assert_eq!(map.get(&5), None);
        assert_eq!(map.get(&6), Some(&"✅".to_owned()));

        let _ = std::fs::remove_file(icon_file(session));
    }

    #[test]
    fn persist_does_not_clobber_another_instances_pane() {
        // Two "instances" persist different panes; the read-merge-write must keep
        // both rather than overwrite the file with a single pane.
        let session = "zjtest_persist_merge";
        let _ = std::fs::remove_file(icon_file(session));

        persist(session, 1, "🤖");
        persist(session, 2, "⏳");

        let map = reload(session);
        assert_eq!(map.len(), 2, "{map:?}");

        let _ = std::fs::remove_file(icon_file(session));
    }

    #[test]
    fn prune_removes_dead_pane_entries() {
        // A pane that dies without firing the `exit` hook leaves its `id=value`
        // line behind forever. prune() drops every key whose id is not in the
        // live set (computed from the live PaneManifest) while keeping the rest.
        let session = "zjtest_prune_dead";
        let _ = std::fs::remove_file(icon_file(session));

        persist(session, 5, "🤖");
        persist(session, 7, "⏳");
        persist(session, 9, "✅");

        let live = BTreeSet::from([5u32, 9u32]); // pane 7 is gone
        prune(session, &live);

        let map = reload(session);
        assert_eq!(map.get(&5), Some(&"🤖".to_owned()));
        assert_eq!(map.get(&9), Some(&"✅".to_owned()));
        assert_eq!(map.get(&7), None, "dead pane 7 was not pruned: {map:?}");
        assert_eq!(map.len(), 2, "{map:?}");

        let _ = std::fs::remove_file(icon_file(session));
    }

    #[test]
    fn prune_with_no_file_is_noop() {
        let session = "zjtest_prune_absent";
        let _ = std::fs::remove_file(icon_file(session));

        let map = prune(session, &BTreeSet::from([1u32, 2u32]));

        assert!(map.is_empty());
        // No file should have been created by pruning an absent session.
        assert!(reload(session).is_empty());
    }

    #[test]
    fn persist_sequential_distinct_panes_both_survive() {
        // The honest counterpart to `concurrent_persist_preserves_an_idle_pane`:
        // two instances persisting DISTINCT panes one-after-another (the common,
        // non-racing case) must both survive read-merge-write. This proves the
        // merge is correct when writes don't interleave; it deliberately does NOT
        // assert anything about truly concurrent distinct-id writes, which can
        // still lose an update (see the doc comment on `persist`).
        let session = "zjtest_persist_sequential_distinct";
        let _ = std::fs::remove_file(icon_file(session));

        persist(session, 1, "🤖"); // instance A's pane
        persist(session, 2, "⏳"); // instance B's pane, written afterwards

        let map = reload(session);
        assert_eq!(map.get(&1), Some(&"🤖".to_owned()));
        assert_eq!(map.get(&2), Some(&"⏳".to_owned()));
        assert_eq!(map.len(), 2, "{map:?}");

        let _ = std::fs::remove_file(icon_file(session));
    }

    #[test]
    fn reload_missing_file_is_empty() {
        let map = reload("zjtest_definitely_absent_session");
        assert!(map.is_empty());
    }

    // Threads are unavailable on wasm32-wasip1 (the default build target), so this
    // concurrency regression only compiles/runs on the host (`--target
    // x86_64-unknown-linux-gnu`).
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn concurrent_persist_preserves_an_idle_pane() {
        // Scope: this exercises idempotent SAME-pane concurrent writes (every
        // thread writes pane 5), proving atomic temp+rename never corrupts the
        // file or drops the untouched idle pane. It does NOT prove safety for
        // concurrent DISTINCT-pane writes — that unlocked read-merge-write race
        // can still lose an update and is documented on `persist` (and covered in
        // the non-racing case by `persist_sequential_distinct_panes_both_survive`).
        //
        // Regression: every per-tab instance re-persists its active pane on each
        // broadcast status change. A non-atomic write (truncate-then-write) lets a
        // concurrent reader observe a half-written file and write it back, dropping
        // an *idle* pane's entry for good (it never re-persists). Atomic temp+rename
        // keeps every read complete.
        let session = "zjtest_concurrent_idle";
        let _ = std::fs::remove_file(icon_file(session));

        persist(session, 999, "✅"); // an idle / finished tab
        persist(session, 5, "🤖"); //   an active pane

        let handles: Vec<_> = (0..16)
            .map(|_| {
                let s = session.to_owned();
                std::thread::spawn(move || {
                    for _ in 0..500 {
                        persist(&s, 5, "🤖");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        let map = reload(session);
        let _ = std::fs::remove_file(icon_file(session));
        assert_eq!(
            map.get(&999),
            Some(&"✅".to_owned()),
            "idle pane was clobbered by concurrent writers: {map:?}"
        );
    }
}
