use std::ops::Sub;

use chrono::{Duration, Local};

use crate::{
    config::{UpdateEventMask, ZellijState},
    widgets::{command::TIMESTAMP_FORMAT, notification},
};

/// Parses the line protocol and updates the state accordingly
///
/// The protocol is as follows:
///
/// zjstatus::command_name::args
///
/// It first starts with `zjstatus` as a prefix to indicate that the line is
/// used for the line protocol and zjstatus should parse it. It is followed
/// by the command name and then the arguments. The following commands are
/// available:
///
/// - `rerun` - Reruns the command with the given name (like in the config) as
///             argument. E.g. `zjstatus::rerun::command_1`
///
/// The function returns a boolean indicating whether the state has been
/// changed and the UI should be re-rendered.
#[tracing::instrument(skip(state))]
pub fn parse_protocol(state: &mut ZellijState, input: &str) -> bool {
    tracing::debug!("parsing protocol");
    let lines = input.split('\n').collect::<Vec<&str>>();

    let mut should_render = false;
    for line in lines {
        let line_renders = process_line(state, line);

        if line_renders {
            should_render = true;
        }
    }

    should_render
}

#[tracing::instrument(skip_all)]
fn process_line(state: &mut ZellijState, line: &str) -> bool {
    let parts = line.split("::").collect::<Vec<&str>>();

    if parts.len() < 3 {
        return false;
    }

    if parts[0] != "zjstatus" {
        return false;
    }

    tracing::debug!("command: {}", parts[1]);

    let mut should_render = false;
    #[allow(clippy::single_match)]
    match parts[1] {
        "rerun" => {
            rerun_command(state, parts[2]);

            should_render = true;
        }
        "notify" => {
            notify(state, parts[2]);

            should_render = true;
        }
        "pipe" => {
            if parts.len() < 4 {
                return false;
            }

            pipe(state, parts[2], parts[3]);

            should_render = true;
        }
        "claude_status" => {
            if parts.len() < 4 {
                return false;
            }

            claude_status(state, parts[2], parts[3]);

            // The icon is rendered inside the `{tabs}` widget, so invalidate the
            // tab cache to force a re-render (the pipe path sets no cache mask).
            state.cache_mask = UpdateEventMask::Tab as u8;

            should_render = true;
        }
        _ => {}
    }

    should_render
}

fn pipe(state: &mut ZellijState, name: &str, content: &str) {
    tracing::debug!("saving pipe result {name} {content}");
    state
        .pipe_results
        .insert(name.to_owned(), content.to_owned());
}

/// Stores a per-pane status value keyed by pane id. An empty value clears the
/// entry. The value is shown on the pane's tab via the `{claude_status}`
/// placeholder in the tabs widget.
fn claude_status(state: &mut ZellijState, pane_id: &str, value: &str) {
    let Ok(pane_id) = pane_id.parse::<u32>() else {
        return;
    };

    if value.is_empty() {
        state.claude_icons.remove(&pane_id);
    } else {
        state.claude_icons.insert(pane_id, value.to_owned());
    }

    // Mirror to the shared per-session file so tabs opened later (which get a
    // fresh, empty plugin instance) can converge on the full icon set.
    if let Some(session) = state.mode.session_name.as_deref() {
        crate::claude_icons::persist(session, pane_id, value);
    }
}

fn notify(state: &mut ZellijState, message: &str) {
    state.incoming_notification = Some(notification::Message {
        body: message.to_string(),
        received_at: Local::now(),
    });
}

fn rerun_command(state: &mut ZellijState, command_name: &str) {
    let command_result = state.command_results.get(command_name);

    if command_result.is_none() {
        return;
    }

    let mut command_result = command_result.unwrap().clone();

    let ts = Sub::<Duration>::sub(Local::now(), Duration::try_days(1).unwrap());

    command_result.context.insert(
        "timestamp".to_string(),
        ts.format(TIMESTAMP_FORMAT).to_string(),
    );

    state.command_results.remove(command_name);
    state
        .command_results
        .insert(command_name.to_string(), command_result.clone());
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::config::{UpdateEventMask, ZellijState};

    #[test]
    fn claude_status_pipe_stores_icon_by_pane_id() {
        let mut state = ZellijState::default();

        let rendered = parse_protocol(&mut state, "zjstatus::claude_status::3::🤖");

        assert!(rendered);
        assert_eq!(state.claude_icons.get(&3), Some(&"🤖".to_owned()));
    }

    #[test]
    fn claude_status_pipe_sets_tab_cache_mask() {
        let mut state = ZellijState::default();

        parse_protocol(&mut state, "zjstatus::claude_status::7::⏳");

        assert_eq!(state.cache_mask, UpdateEventMask::Tab as u8);
    }

    #[test]
    fn claude_status_pipe_empty_value_clears_icon() {
        let mut state = ZellijState::default();
        state.claude_icons.insert(3, "🤖".to_owned());

        parse_protocol(&mut state, "zjstatus::claude_status::3::");

        assert_eq!(state.claude_icons.get(&3), None);
    }

    #[test]
    fn claude_status_pipe_ignores_non_numeric_pane_id() {
        let mut state = ZellijState::default();

        parse_protocol(&mut state, "zjstatus::claude_status::abc::🤖");

        assert!(state.claude_icons.is_empty());
    }
}
