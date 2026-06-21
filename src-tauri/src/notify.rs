//! Desktop notification (+ optional sound) when a session finishes.
//!
//! Gated by [`Config`]; multiple completions each notify (never coalesced).

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::config::Config;
use crate::state::SessionView;
use csm_core::{SessionKey, Source};

fn source_label(s: Source) -> &'static str {
    match s {
        Source::ClaudeCode => "Claude Code",
        Source::Codex => "Codex",
    }
}

/// Fire a completion notification for `key`, looking up its view in `snapshot`.
pub fn on_completed(app: &AppHandle, key: &SessionKey, snapshot: &[SessionView], config: &Config) {
    if !config.notifications {
        return;
    }
    let lang = crate::i18n::resolve(&config.language);
    let view = snapshot.iter().find(|v| &v.key == key);
    let (title, body) = match view {
        Some(v) => (
            lang.notify_finished_title(source_label(v.source)),
            format!(
                "{}{}",
                v.cwd,
                if v.host.is_empty() {
                    String::new()
                } else {
                    format!("  ·  {}", v.host)
                }
            ),
        ),
        None => (
            lang.notify_finished_title_unknown(),
            source_label(key.source).to_string(),
        ),
    };

    // Sound on Windows toasts is controlled by the OS; `config.sound` reserved
    // for an explicit sound channel in a later iteration.
    let _ = config.sound;

    let _ = app.notification().builder().title(title).body(body).show();
}

/// Fire a "waiting for your input/approval" notification for `key`.
pub fn on_awaiting_input(
    app: &AppHandle,
    key: &SessionKey,
    snapshot: &[SessionView],
    config: &Config,
) {
    if !config.notifications {
        return;
    }
    let lang = crate::i18n::resolve(&config.language);
    let view = snapshot.iter().find(|v| &v.key == key);
    let (title, body) = match view {
        Some(v) => (
            lang.notify_waiting_title(source_label(v.source)),
            lang.notify_waiting_body(&v.cwd),
        ),
        None => (
            lang.notify_waiting_title(source_label(key.source)),
            lang.notify_waiting_body(""),
        ),
    };
    let _ = app.notification().builder().title(title).body(body).show();
}
