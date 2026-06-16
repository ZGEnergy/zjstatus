//! Cross-instance sharing for the `{claude_status}` per-tab icons.
//!
//! zjstatus is loaded once per tab (via `default_tab_template`), so a
//! `claude_status` pipe only reaches the instances that exist at that moment —
//! a tab opened later starts with an empty icon map. To converge, every instance
//! mirrors its per-pane icon map to a per-session file and reloads it on the
//! events all instances receive (PaneUpdate/TabUpdate/Timer). Writes are
//! read-merge-write so concurrent instances don't clobber each other's panes.

use std::collections::BTreeMap;
use std::path::PathBuf;

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

/// Apply a single pane's status change to the shared session file. Read-merge-
/// write so a concurrent instance's panes are preserved; an empty value clears
/// the pane. All errors are ignored — the icon is best-effort cosmetic state.
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
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, serialize(&map));
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
    use super::{icon_file, parse, persist, reload, serialize};
    use std::collections::BTreeMap;

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
    fn reload_missing_file_is_empty() {
        let map = reload("zjtest_definitely_absent_session");
        assert!(map.is_empty());
    }
}
