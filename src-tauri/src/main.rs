//! CLI Session Monitor — desktop app entry point (Tauri v2).
//!
//! Wires the file-bus `Source` -> `StateMachine` -> `sessions:update` events and
//! completion notifications, plus a tray icon and the install/config IPC commands.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod applog;
mod config;
mod demo;
mod i18n;
mod notify;
mod state;

use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

use config::Config;
use csm_core::installer::{self, InstallOutcome};
use csm_core::Event;
use csm_core::Source as CliSource;
use csm_watch::{CodexRolloutSource, FsWatchSource, Source};
use state::{Effect, SessionView, StateMachine};

struct AppState {
    sm: Mutex<StateMachine>,
    config: Mutex<Config>,
    /// Channel sender for the event loop, so the relay subscription can be
    /// (re)started live when the user changes relay settings.
    relay_tx: Mutex<Option<std::sync::mpsc::Sender<Event>>>,
    /// Stop flag for the current relay subscription thread (toggled on change).
    relay_stop: Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    /// Whether the relay subscription stream is currently connected (for the
    /// Settings indicator). Shared with the running NtfySource.
    relay_connected: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Remembered editor per directory (normalized cwd -> "cursor"/"code"),
    /// learned when a window is matched, so reopening a closed session uses the
    /// right editor instead of trying all of them.
    editor_map: Mutex<HashMap<String, String>>,
}

/// (Re)start the ntfy relay subscription from the current config: stop any
/// existing one, then spawn a fresh thread for the new topic (if set). Lets the
/// user change the relay topic in Settings without restarting the app.
fn start_relay(app: &AppHandle, cfg: &Config) {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    let state = app.state::<AppState>();
    use std::sync::atomic::Ordering;
    // Signal the previous subscription (if any) to exit.
    if let Some(flag) = state.relay_stop.lock().unwrap().take() {
        flag.store(true, Ordering::SeqCst);
    }
    state.relay_connected.store(false, Ordering::SeqCst);
    if cfg.relay_topic.trim().is_empty() {
        return;
    }
    let Some(tx) = state.relay_tx.lock().unwrap().clone() else {
        return;
    };
    let stop = Arc::new(AtomicBool::new(false));
    *state.relay_stop.lock().unwrap() = Some(stop.clone());
    let connected = state.relay_connected.clone();
    let (base, topic) = (cfg.relay_url.clone(), cfg.relay_topic.clone());
    let token = Some(cfg.relay_token.clone()).filter(|t| !t.is_empty());
    thread::spawn(move || {
        csm_watch::NtfySource {
            base,
            topic,
            token,
            since: Some("5m".to_string()),
            stop,
            connected: Some(connected),
        }
        .run(tx);
    });
}

/// Events older than this (by their own `ts`) don't trigger notifications/flash
/// — they're catch-up replays from the relay, not things happening right now.
const NOTIFY_GRACE_MS: i64 = 60_000;

// Rescan fairly often so a session that's open & waiting (e.g. paused at the
// resume-session prompt, where no hook fires) shows up promptly. The look-back
// window (how far back to surface idle sessions) is config.discover_window_days.
const DISCOVER_INTERVAL_SECS: u64 = 30;

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------- IPC commands ----------------

#[tauri::command]
fn get_snapshot(state: State<AppState>) -> Vec<SessionView> {
    state.sm.lock().unwrap().snapshot()
}

#[tauri::command]
fn get_config(state: State<AppState>) -> Config {
    // Always report the true config: overriding `lightweight` here (as the demo
    // once did) desyncs the Settings checkbox from the live state, so the toggle
    // can't be turned off. The demo simply respects the user's lightweight setting.
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn set_config(app: AppHandle, state: State<AppState>, mut config: Config) -> Config {
    let old = state.config.lock().unwrap().clone();
    // These fields are owned by the backend / other commands, NOT the settings
    // panel: session names & remembered resume commands (edited on the cards via
    // set_session_name/set_session_cmd) and the window position/size (save_window_*).
    // The panel sends a snapshot taken when it opened, which can be stale (e.g. a
    // card was renamed afterwards) — so never let it clobber these. Keep the
    // backend's current values instead.
    config.session_names = old.session_names.clone();
    config.session_cmds = old.session_cmds.clone();
    config.win_x = old.win_x;
    config.win_y = old.win_y;
    config.panel_w = old.panel_w;
    config.panel_h = old.panel_h;
    // persist (best-effort), archiving the previous config first
    let _ = config.save_with_backup(Config::default_path());
    state
        .sm
        .lock()
        .unwrap()
        .set_idle_threshold_secs(config.idle_threshold_secs);
    apply_window_prefs(&app, &config);
    *state.config.lock().unwrap() = config.clone();
    // Relay settings changed -> restart the subscription live (no app restart).
    if old.relay_url != config.relay_url
        || old.relay_topic != config.relay_topic
        || old.relay_token != config.relay_token
    {
        start_relay(&app, &config);
    }
    config
}

/// Persist the widget's current position (physical px) so it reappears there on
/// the next launch. Lightweight: only touches win_x/win_y, no window-pref side
/// effects (unlike `set_config`).
#[tauri::command]
fn save_window_pos(state: State<AppState>, x: i32, y: i32) {
    let mut cfg = state.config.lock().unwrap();
    cfg.win_x = Some(x);
    cfg.win_y = Some(y);
    let _ = cfg.save_to(Config::default_path());
}

/// Set (or clear, when `name` is blank) a user-assigned display name for a
/// session, keyed by its session id so it survives restarts and `--resume`.
/// Lightweight persist, like `save_window_pos`.
#[tauri::command]
fn set_session_name(state: State<AppState>, id: String, name: String) {
    let mut cfg = state.config.lock().unwrap();
    let name = name.trim();
    if name.is_empty() {
        cfg.session_names.remove(&id);
    } else {
        cfg.session_names.insert(id, name.to_string());
    }
    let _ = cfg.save_with_backup(Config::default_path());
}

/// Set (or clear, when blank) the resume command a card remembers, keyed by
/// session id — so flags like `--yolo` / `--dangerously-skip-permissions` that
/// the default command drops are preserved. Lightweight persist.
#[tauri::command]
fn set_session_cmd(state: State<AppState>, id: String, cmd: String) {
    let mut cfg = state.config.lock().unwrap();
    let cmd = cmd.trim();
    if cmd.is_empty() {
        cfg.session_cmds.remove(&id);
    } else {
        cfg.session_cmds.insert(id, cmd.to_string());
    }
    let _ = cfg.save_with_backup(Config::default_path());
}

/// Persist the user-chosen full-panel size (logical px) so a resized panel is
/// remembered next launch. Lightweight, like `save_window_pos`.
#[tauri::command]
fn save_window_size(state: State<AppState>, w: u32, h: u32) {
    let mut cfg = state.config.lock().unwrap();
    cfg.panel_w = w;
    cfg.panel_h = h;
    let _ = cfg.save_to(Config::default_path());
}

/// Close (hide) a session card. It reappears if the session shows fresh activity.
#[tauri::command]
fn dismiss_session(app: AppHandle, state: State<AppState>, key: csm_core::SessionKey) {
    let snap = {
        let mut sm = state.sm.lock().unwrap();
        if sm.dismiss(&key) {
            Some(sm.snapshot())
        } else {
            None
        }
    };
    if let Some(s) = snap {
        let _ = app.emit("sessions:update", &s);
    }
}

/// Which session card(s) the current foreground window belongs to. `own` means
/// our own widget is focused (caller keeps the previous highlight); otherwise
/// `keys` are the cards whose matched editor window is the foreground (empty =
/// clear highlight).
#[derive(serde::Serialize)]
struct ActiveCards {
    own: bool,
    keys: Vec<csm_core::SessionKey>,
}

/// Payload for the `session:flash` event: which session changed and how, so the
/// UI can pulse the right card ("done" = turn finished, "waiting" = awaiting your
/// choice/approval).
#[derive(Clone, serde::Serialize)]
struct FlashPayload<'a> {
    kind: &'a str,
    key: &'a csm_core::SessionKey,
}

#[cfg(windows)]
#[tauri::command]
fn active_window_cards(window: tauri::WebviewWindow, state: State<AppState>) -> ActiveCards {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let fg = unsafe { GetForegroundWindow() };
    if fg.0.is_null() {
        return ActiveCards {
            own: false,
            keys: Vec::new(),
        };
    }
    if let Ok(h) = window.hwnd() {
        if fg == HWND(h.0 as _) {
            return ActiveCards {
                own: true,
                keys: Vec::new(),
            }; // keep last highlight
        }
    }
    let wins = enumerate_top_windows();
    let Some(fg_idx) = wins.iter().position(|(h, _)| *h == fg) else {
        return ActiveCards {
            own: false,
            keys: Vec::new(),
        };
    };
    // A card is active iff its matched editor window — the SAME one shown on the
    // card and used for jump — is the foreground window. Reusing best_editor_window
    // keeps it host-aware, so a Remote-SSH window matches its remote session too,
    // and since each card resolves to one window, only the card(s) you're actually
    // in light up. Idle sessions are never highlighted.
    let snap = state.sm.lock().unwrap().snapshot();
    let hints = cli_editor_hints();
    let keys = snap
        .iter()
        .filter(|v| v.status != state::SessionStatus::Idle)
        .filter(|v| {
            let prefer = hints.get(&normalize_dir(&v.cwd)).copied();
            best_editor_window(&v.cwd, &v.host, &wins, prefer) == Some(fg_idx)
        })
        .map(|v| v.key.clone())
        .collect();
    ActiveCards { own: false, keys }
}

#[cfg(not(windows))]
#[tauri::command]
fn active_window_cards(_state: State<AppState>) -> ActiveCards {
    ActiveCards {
        own: false,
        keys: Vec::new(),
    }
}

/// Set position and size in a single atomic op so the window never grows at the
/// old spot and then jumps (which flickers, esp. when expanding from a
/// bottom/right edge). All values are physical px.
#[cfg(windows)]
#[tauri::command]
fn set_window_bounds(window: tauri::WebviewWindow, x: i32, y: i32, w: i32, h: i32) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
    if let Ok(h_) = window.hwnd() {
        unsafe {
            let _ = SetWindowPos(
                HWND(h_.0 as _),
                None,
                x,
                y,
                w,
                h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

#[cfg(not(windows))]
#[tauri::command]
fn set_window_bounds(window: tauri::WebviewWindow, x: i32, y: i32, w: i32, h: i32) {
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    let _ = window.set_size(tauri::PhysicalSize::new(w.max(1) as u32, h.max(1) as u32));
}

/// The local hostname, so the UI can tell local sessions from remote ones.
#[tauri::command]
fn local_host() -> String {
    if demo::enabled() {
        return demo::HOST.to_string();
    }
    csm_core::paths::host_name()
}

/// `window.title` we set so the title carries the full folder path — this lets
/// the jump-to-window matcher disambiguate two windows whose folder basenames
/// collide (e.g. .../frontend/app vs .../backend/app).
const WINDOW_TITLE_VALUE: &str =
    "${dirty}${folderPath}${separator}${remoteName}${separator}${appName}";

#[cfg(windows)]
fn editor_settings_paths() -> Vec<(&'static str, std::path::PathBuf)> {
    let mut v = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        let base = std::path::Path::new(&appdata);
        v.push((
            "VS Code",
            base.join("Code").join("User").join("settings.json"),
        ));
        v.push((
            "Cursor",
            base.join("Cursor").join("User").join("settings.json"),
        ));
    }
    v
}
#[cfg(not(windows))]
fn editor_settings_paths() -> Vec<(&'static str, std::path::PathBuf)> {
    Vec::new()
}

/// One-click: write `window.title` into VS Code / Cursor user settings so jump
/// can tell same-named folders apart. Backup-first; never clobbers an existing
/// `window.title` the user already set.
#[tauri::command]
fn optimize_editor_jump(state: State<AppState>) -> Result<String, String> {
    let lang = i18n::resolve(&state.config.lock().unwrap().language);
    let targets = editor_settings_paths();
    if targets.is_empty() {
        return Err(lang.jump_only_windows().into());
    }
    let mut msgs = Vec::new();
    let mut changed = false;
    let mut found = false;
    for (name, path) in targets {
        if !path.exists() {
            continue; // editor not installed
        }
        found = true;
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                msgs.push(lang.jump_read_failed(&name, &e.to_string()));
                continue;
            }
        };
        if text.contains("\"window.title\"") {
            msgs.push(lang.jump_has_title(&name));
            continue;
        }
        let backup = path.with_extension("json.csm-bak");
        if std::fs::write(&backup, &text).is_err() {
            msgs.push(lang.jump_backup_failed(&name));
            continue;
        }
        let line = format!("\n  \"window.title\": \"{WINDOW_TITLE_VALUE}\",");
        let new = match text.find('{') {
            Some(pos) => format!("{}{}{}", &text[..=pos], line, &text[pos + 1..]),
            None => format!("{{{line}\n}}\n"),
        };
        match std::fs::write(&path, new) {
            Ok(_) => {
                changed = true;
                msgs.push(lang.jump_written(&name, &backup.display().to_string()));
            }
            Err(e) => msgs.push(lang.jump_write_failed(&name, &e.to_string())),
        }
    }
    if !found {
        return Err(lang.jump_no_settings().into());
    }
    let _ = changed;
    Ok(msgs.join("\n"))
}

#[derive(serde::Serialize)]
struct EditorJumpStatus {
    /// True when every installed editor has a window.title carrying the path.
    configured: bool,
    summary: String,
}

/// Report whether VS Code / Cursor are set up for accurate jump (a window.title
/// that includes the folder path).
#[tauri::command]
fn editor_jump_status(state: State<AppState>) -> EditorJumpStatus {
    let lang = i18n::resolve(&state.config.lock().unwrap().language);
    let mut installed = 0;
    let mut ok = 0;
    let mut parts = Vec::new();
    for (name, path) in editor_settings_paths() {
        if !path.exists() {
            continue;
        }
        installed += 1;
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let lt = text.to_lowercase();
        if lt.contains("\"window.title\"") && lt.contains("folderpath") {
            ok += 1;
            parts.push(lang.jump_status_configured(&name));
        } else if lt.contains("\"window.title\"") {
            parts.push(lang.jump_status_custom(&name));
        } else {
            parts.push(lang.jump_status_unconfigured(&name));
        }
    }
    EditorJumpStatus {
        configured: installed > 0 && ok == installed,
        summary: if parts.is_empty() {
            lang.jump_none_detected().into()
        } else {
            parts.join("  |  ")
        },
    }
}

/// Revert: remove the `window.title` line we added (only ours).
#[tauri::command]
fn revert_editor_jump(state: State<AppState>) -> Result<String, String> {
    let lang = i18n::resolve(&state.config.lock().unwrap().language);
    let mut msgs = Vec::new();
    let mut found = false;
    for (name, path) in editor_settings_paths() {
        if !path.exists() {
            continue;
        }
        found = true;
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !text.contains(WINDOW_TITLE_VALUE) {
            msgs.push(lang.revert_nothing(&name));
            continue;
        }
        let new: Vec<&str> = text
            .lines()
            .filter(|l| !l.contains(WINDOW_TITLE_VALUE))
            .collect();
        match std::fs::write(&path, new.join("\n")) {
            Ok(_) => msgs.push(lang.revert_done(&name)),
            Err(e) => msgs.push(lang.revert_failed(&name, &e.to_string())),
        }
    }
    if !found {
        return Err(lang.jump_no_settings().into());
    }
    Ok(msgs.join("\n"))
}

/// Jump to a session's editor window. First tries to focus an already-open
/// Cursor/VS Code window matching this session — this covers BOTH local sessions
/// and remote ones opened via VS Code/Cursor Remote-SSH (the window runs on this
/// machine even though the session host is remote). Only if that fails AND the
/// folder exists locally do we open it fresh.
#[tauri::command]
fn open_session(
    state: State<AppState>,
    path: String,
    host: String,
    create: bool,
) -> Result<(), String> {
    if focus_editor_window(&path, &host) {
        return Ok(());
    }
    if create && !path.trim().is_empty() && std::path::Path::new(&path).exists() {
        // Open the SPECIFIC editor remembered for this dir (so a closed session
        // reopens in the editor it was in, not a guess) — single editor, no
        // try-everything chain.
        let preferred = state
            .editor_map
            .lock()
            .unwrap()
            .get(&normalize_dir(&path))
            .cloned();
        return open_folder(&path, preferred.as_deref());
    }
    Err("No matching editor window (closed, or a session on another host)".into())
}

/// True if `name` resolves on PATH (so we can launch exactly the editor that
/// exists, rather than launching several and hoping).
#[cfg(windows)]
fn cmd_on_path(name: &str) -> bool {
    use std::process::Command;
    Command::new("cmd")
        .args(["/C", "where", name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Open `path` in ONE editor: the preferred (remembered) one if present, else the
/// first of cursor/code that exists, else the file manager. Launches without
/// waiting (so a non-zero exit can't make it fall through and open several).
#[cfg(windows)]
fn open_folder(path: &str, preferred: Option<&str>) -> Result<(), String> {
    use std::process::Command;
    let mut order: Vec<&str> = Vec::new();
    if let Some(p) = preferred {
        order.push(p);
    }
    for e in ["cursor", "code"] {
        if !order.contains(&e) {
            order.push(e);
        }
    }
    for editor in order {
        if cmd_on_path(editor) {
            let _ = Command::new("cmd").args(["/C", editor, path]).spawn();
            return Ok(());
        }
    }
    Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn open_folder(path: &str, preferred: Option<&str>) -> Result<(), String> {
    use std::process::Command;
    let mut order: Vec<&str> = Vec::new();
    if let Some(p) = preferred {
        order.push(p);
    }
    for e in ["cursor", "code"] {
        if !order.contains(&e) {
            order.push(e);
        }
    }
    for editor in order {
        if Command::new(editor).arg(path).spawn().is_ok() {
            return Ok(());
        }
    }
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Enumerate visible top-level windows as (handle, title).
#[cfg(windows)]
fn enumerate_top_windows() -> Vec<(windows::Win32::Foundation::HWND, String)> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };
    extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
                return BOOL(1);
            }
            let mut buf = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut buf);
            if n > 0 {
                let title = String::from_utf16_lossy(&buf[..n as usize]);
                let v = &mut *(lparam.0 as *mut Vec<(HWND, String)>);
                v.push((hwnd, title));
            }
            BOOL(1)
        }
    }
    let mut wins: Vec<(HWND, String)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut wins as *mut _ as isize));
    }
    wins
}

/// Map a local CLI's launch dir -> the editor it's running under ("cursor" /
/// "code"), by walking each running `codex.exe` / `claude.exe` up its parent-
/// process chain until it hits a `Cursor.exe` / `Code.exe` ancestor (its
/// integrated terminal's host). Lets jump/highlight pick the right window when the
/// SAME folder is open in both editors at once. Local only — a remote session's
/// CLI isn't a process on this machine (it's matched by host instead).
#[cfg(windows)]
fn cli_editor_hints() -> HashMap<String, &'static str> {
    cli_snapshot().editor_hints.clone()
}

/// Among `wins`, pick the index of the Cursor/VS Code window best matching this
/// session (folder names from `cwd`; host for Remote-SSH). None = no confident
/// unique match. `prefer` ("cursor"/"code") breaks an editor tie when the same
/// folder is open in both. Shared by jump-to-window and the per-card window label.
#[cfg(windows)]
fn best_editor_window(
    cwd: &str,
    host: &str,
    wins: &[(windows::Win32::Foundation::HWND, String)],
    prefer: Option<&str>,
) -> Option<usize> {
    // Match by the cwd PATH-TAIL, not individual folder names: score each window
    // by the longest *suffix* of the cwd path its title contains (titles carry
    // the folder path when "Optimize jump" is on). This way a shared parent
    // (e.g. ".../projects/{web-app,api-server}") can't tie two sibling windows —
    // only the deepest, most-specific tail wins.
    let cwd_n = normalize_dir(cwd); // lowercase, '\'-separated, no trailing slash
    let comps: Vec<&str> = cwd_n.split('\\').filter(|c| !c.is_empty()).collect();
    let host_l = host.to_lowercase();
    let is_remote = !host_l.is_empty() && host_l != csm_core::paths::host_name().to_lowercase();
    if comps.is_empty() && host_l.len() < 3 {
        return None;
    }
    // (index, path_match_len, host_present)
    let mut scored: Vec<(usize, usize, bool)> = Vec::new();
    for (i, (_, title)) in wins.iter().enumerate() {
        let lt = title.to_lowercase().replace('/', "\\");
        if !(lt.contains("cursor") || lt.contains("visual studio code")) {
            continue;
        }
        let host_present = host_l.len() >= 3 && lt.contains(&host_l);

        // The window's opened folder is the leading title token (e.g.
        // "d:\projects\web-app" in "D:\projects\web-app - Cursor").
        let cut = lt
            .find(" - ")
            .or_else(|| lt.find(" \u{2014} "))
            .unwrap_or(lt.len());
        let winfolder = lt[..cut].trim().trim_end_matches('\\');
        // Relate the window's folder to the session cwd (works when the editor
        // is opened at the cwd, an ANCESTOR of it, or a descendant). Deeper /
        // more-specific overlap scores higher.
        let rel = if winfolder.len() >= 3 {
            if winfolder == cwd_n {
                cwd_n.len()
            } else if cwd_n.starts_with(&format!("{winfolder}\\")) {
                winfolder.len() // window opened at an ancestor of cwd
            } else if winfolder.starts_with(&format!("{cwd_n}\\")) {
                cwd_n.len() // window opened at a descendant of cwd
            } else {
                0
            }
        } else {
            0
        };
        // Fallback: longest cwd path-suffix appearing anywhere in the title
        // (covers VS Code's default "file - folder - app" titles).
        let mut suffix = 0;
        for start in 0..comps.len() {
            let s = comps[start..].join("\\");
            if s.len() >= 3 && lt.contains(&s) {
                suffix = s.len();
                break;
            }
        }
        let score = rel.max(suffix);
        if score > 0 || host_present {
            scored.push((i, score, host_present));
        }
    }
    if scored.is_empty() {
        return None;
    }
    let mut pool: Vec<&(usize, usize, bool)> = if is_remote {
        // A remote session may ONLY match a window that carries its own host name
        // (a VS Code/Cursor Remote-SSH window). Never fall back to a same-named
        // LOCAL window — that's the "jumps to the wrong window" bug for a session
        // running on another machine (e.g. Codex inside a VM, no local window).
        let host_pool: Vec<&(usize, usize, bool)> = scored.iter().filter(|s| s.2).collect();
        if host_pool.is_empty() {
            return None;
        }
        host_pool
    } else {
        scored.iter().collect()
    };
    pool.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
    let best = *pool[0];
    let tied: Vec<usize> = pool
        .iter()
        .filter(|s| s.1 == best.1 && s.2 == best.2)
        .map(|s| s.0)
        .collect();
    if tied.len() > 1 {
        // Same folder open in more than one editor → ambiguous. If we know which
        // editor this session's CLI is actually running under (from its process
        // ancestry), break the tie by preferring that editor's window.
        if let Some(ed) = prefer {
            let marker = if ed == "cursor" {
                "cursor"
            } else {
                "visual studio code"
            };
            let matching: Vec<usize> = tied
                .iter()
                .copied()
                .filter(|&i| wins[i].1.to_lowercase().contains(marker))
                .collect();
            if matching.len() == 1 {
                return Some(matching[0]);
            }
        }
        return None;
    }
    if best.1 == 0 && !best.2 {
        return None;
    }
    Some(best.0)
}

/// All visible window titles (for diagnostics). Windows-only.
#[cfg(windows)]
fn all_visible_titles() -> Vec<String> {
    enumerate_top_windows()
        .into_iter()
        .map(|(_, t)| t)
        .collect()
}
#[cfg(not(windows))]
fn all_visible_titles() -> Vec<String> {
    Vec::new()
}

/// Best-effort: the window handle matching this session, or None. Logs the cwd,
/// the candidate editor window titles, and the decision so failures are
/// diagnosable from `~/.cli-session-monitor/csm.log`.
#[cfg(windows)]
fn find_editor_window(cwd: &str, host: &str) -> Option<windows::Win32::Foundation::HWND> {
    let wins = enumerate_top_windows();
    let hints = cli_editor_hints();
    let prefer = hints.get(&normalize_dir(cwd)).copied();
    let idx = best_editor_window(cwd, host, &wins, prefer);
    if applog::enabled() {
        applog::line(&format!("[jump] cwd={cwd:?} host={host:?}"));
        for (_, t) in wins.iter().filter(|(_, t)| {
            let l = t.to_lowercase();
            l.contains("cursor") || l.contains("visual studio code")
        }) {
            applog::line(&format!("[jump]   candidate: {t}"));
        }
        match idx {
            Some(i) => applog::line(&format!("[jump]   -> matched: {}", wins[i].1)),
            None => applog::line("[jump]   -> NO unique match"),
        }
    }
    idx.map(|i| wins[i].0)
}

/// Bring the matched editor window to the foreground (un-minimizing only if
/// needed — never shrinking a maximized window). Returns true if focused.
#[cfg(windows)]
fn focus_editor_window(cwd: &str, host: &str) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };
    match find_editor_window(cwd, host) {
        Some(hwnd) => unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd).as_bool()
        },
        None => false,
    }
}

#[cfg(not(windows))]
fn focus_editor_window(_cwd: &str, _host: &str) -> bool {
    false
}

/// Normalize a directory for comparison. Delegates to the shared implementation
/// so the app and the state machine normalize identically (single source).
fn normalize_dir(p: &str) -> String {
    csm_core::pathmatch::normalize_dir(p)
}

/// Working directories of running `codex.exe` / `claude.exe` processes. A CLI is
/// an interactive process that stays alive between turns, so this reliably tells
/// whether a session is actually open (unlike a file's mtime). Dirs normalized.
/// A snapshot of the running CLI processes: their launch dirs (for discovery /
/// is-it-open) and the editor each is hosted in (for jump tie-breaking). Built in
/// ONE process-table walk and cached briefly, so the ~1.2s foreground poll and the
/// 30s discovery scan don't each re-enumerate every process + read each CLI's PEB.
#[cfg(windows)]
struct CliSnapshot {
    dirs: Vec<(CliSource, String)>,
    editor_hints: HashMap<String, &'static str>,
}

#[cfg(windows)]
static CLI_CACHE: Mutex<Option<(i64, std::sync::Arc<CliSnapshot>)>> = Mutex::new(None);

/// Cached `CliSnapshot` (rebuilt at most every `TTL_MS`).
#[cfg(windows)]
fn cli_snapshot() -> std::sync::Arc<CliSnapshot> {
    const TTL_MS: i64 = 1500;
    let now = now_ms();
    if let Some((ts, snap)) = CLI_CACHE.lock().unwrap().as_ref() {
        if now - *ts < TTL_MS {
            return snap.clone();
        }
    }
    let snap = std::sync::Arc::new(build_cli_snapshot());
    *CLI_CACHE.lock().unwrap() = Some((now, snap.clone()));
    snap
}

#[cfg(windows)]
fn build_cli_snapshot() -> CliSnapshot {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    // One pass: pid -> (exe name lowercased, parent pid), plus each running CLI.
    let mut procs: HashMap<u32, (String, u32)> = HashMap::new();
    let mut clis: Vec<(u32, CliSource, String)> = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return CliSnapshot {
                dirs: Vec::new(),
                editor_hints: HashMap::new(),
            };
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let end = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]).to_lowercase();
                let source = match name.as_str() {
                    "codex.exe" => Some(CliSource::Codex),
                    "claude.exe" => Some(CliSource::ClaudeCode),
                    _ => None,
                };
                if let Some(src) = source {
                    if let Some(cwd) = process_cwd(entry.th32ProcessID) {
                        clis.push((entry.th32ProcessID, src, normalize_dir(&cwd)));
                    }
                }
                procs.insert(entry.th32ProcessID, (name, entry.th32ParentProcessID));
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    // Launch dirs + the editor each CLI runs under (walk its parent chain to a
    // Cursor.exe / Code.exe ancestor hosting its integrated terminal).
    let mut dirs = Vec::with_capacity(clis.len());
    let mut editor_hints: HashMap<String, &'static str> = HashMap::new();
    for (pid, src, dir) in clis {
        dirs.push((src, dir.clone()));
        let mut cur = pid;
        for _ in 0..24 {
            let Some((name, ppid)) = procs.get(&cur) else {
                break;
            };
            if name == "cursor.exe" {
                editor_hints.insert(dir, "cursor");
                break;
            }
            if name == "code.exe" {
                editor_hints.insert(dir, "code");
                break;
            }
            if *ppid == 0 || *ppid == cur {
                break;
            }
            cur = *ppid;
        }
    }
    CliSnapshot { dirs, editor_hints }
}

#[cfg(windows)]
fn running_cli_dirs() -> Vec<(CliSource, String)> {
    cli_snapshot().dirs.clone()
}

/// Read a process's current working directory from its PEB (x64). Returns None
/// if not accessible. Offsets: PEB+0x20 = ProcessParameters; params+0x38 =
/// CurrentDirectory (UNICODE_STRING: Length@0, Buffer@0x8).
#[cfg(windows)]
fn process_cwd(pid: u32) -> Option<String> {
    use std::ffi::c_void;
    use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_BASIC_INFORMATION, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    unsafe {
        let h = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()?;
        let read = |addr: usize, buf: *mut c_void, size: usize| -> Option<()> {
            let mut got = 0usize;
            ReadProcessMemory(h, addr as *const c_void, buf, size, Some(&mut got)).ok()?;
            (got == size).then_some(())
        };
        let result = (|| {
            let mut pbi = PROCESS_BASIC_INFORMATION::default();
            let mut retlen = 0u32;
            if NtQueryInformationProcess(
                h,
                ProcessBasicInformation,
                &mut pbi as *mut _ as *mut c_void,
                std::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
                &mut retlen,
            )
            .ok()
            .is_err()
            {
                return None;
            }
            let peb = pbi.PebBaseAddress as usize;
            if peb == 0 {
                return None;
            }
            let mut params: usize = 0;
            read(peb + 0x20, &mut params as *mut _ as *mut c_void, 8)?;
            let mut len: u16 = 0;
            read(params + 0x38, &mut len as *mut _ as *mut c_void, 2)?;
            let mut buf_ptr: usize = 0;
            read(params + 0x40, &mut buf_ptr as *mut _ as *mut c_void, 8)?;
            if len == 0 || buf_ptr == 0 {
                return None;
            }
            // Round the u16 count UP so an odd byte-length can't make
            // ReadProcessMemory write 1 byte past the buffer; decode only the
            // whole u16s.
            let count = (len as usize + 1) / 2;
            let mut w = vec![0u16; count];
            read(buf_ptr, w.as_mut_ptr() as *mut c_void, len as usize)?;
            Some(String::from_utf16_lossy(&w[..(len as usize) / 2]))
        })();
        let _ = CloseHandle(h);
        result
    }
}

#[cfg(not(windows))]
fn running_cli_dirs() -> Vec<(CliSource, String)> {
    Vec::new()
}

#[derive(serde::Deserialize)]
struct SessionLoc {
    cwd: String,
    host: String,
}

/// For each session, the title of the Cursor/VS Code window it maps to (or null).
/// Enumerates windows once; also learns which editor each dir uses (for reopen).
#[tauri::command]
fn session_window_titles(state: State<AppState>, sessions: Vec<SessionLoc>) -> Vec<Option<String>> {
    if demo::enabled() {
        // Synthesize plausible window titles so demo cards show a 🪟 line.
        return sessions
            .iter()
            .map(|s| demo::window_title(&s.cwd, &s.host))
            .collect();
    }
    let (titles, learned) = match_session_windows(&sessions);
    if !learned.is_empty() {
        let mut map = state.editor_map.lock().unwrap();
        for (dir, editor) in learned {
            map.insert(dir, editor);
        }
    }
    titles
}

/// Editor kind from a window title.
fn editor_from_title(title: &str) -> Option<&'static str> {
    let lt = title.to_lowercase();
    if lt.contains("cursor") {
        Some("cursor")
    } else if lt.contains("visual studio code") {
        Some("code")
    } else {
        None
    }
}

/// Returns (titles per session, learned (dir, editor) pairs to remember).
#[cfg(windows)]
fn match_session_windows(sessions: &[SessionLoc]) -> (Vec<Option<String>>, Vec<(String, String)>) {
    let wins = enumerate_top_windows();
    let hints = cli_editor_hints();
    let mut titles = Vec::with_capacity(sessions.len());
    let mut learned = Vec::new();
    for s in sessions {
        let prefer = hints.get(&normalize_dir(&s.cwd)).copied();
        match best_editor_window(&s.cwd, &s.host, &wins, prefer) {
            Some(i) => {
                let title = wins[i].1.clone();
                if let Some(ed) = editor_from_title(&title) {
                    learned.push((normalize_dir(&s.cwd), ed.to_string()));
                }
                titles.push(Some(title));
            }
            None => titles.push(None),
        }
    }
    (titles, learned)
}

#[cfg(not(windows))]
fn match_session_windows(sessions: &[SessionLoc]) -> (Vec<Option<String>>, Vec<(String, String)>) {
    (sessions.iter().map(|_| None).collect(), Vec::new())
}

#[derive(serde::Serialize)]
struct IntegrationStatus {
    claude: InstallOutcome,
    codex: InstallOutcome,
}

#[tauri::command]
fn integration_status() -> Result<IntegrationStatus, String> {
    let claude = installer::claude::status(&csm_core::paths::claude_settings_path())
        .map_err(|e| e.to_string())?;
    let codex = installer::codex::status(&csm_core::paths::codex_config_path())
        .map_err(|e| e.to_string())?;
    Ok(IntegrationStatus { claude, codex })
}

#[tauri::command]
fn install_integration(target: String) -> Result<InstallOutcome, String> {
    let reporter = installer::reporter_path();
    match target.as_str() {
        "claude" => installer::claude::install(&csm_core::paths::claude_settings_path(), &reporter),
        "codex" => installer::codex::install(&csm_core::paths::codex_config_path(), &reporter),
        other => return Err(format!("unknown target: {other}")),
    }
    .map_err(|e| e.to_string())
}

/// Generate a ready-to-run shell script that launches `csm-agent` on a remote
/// Linux box, with this app's relay topic/url/token baked in. The user copies
/// the file to the server and runs `bash remote-agent.sh`.
#[tauri::command]
fn export_agent_script(state: State<AppState>) -> Result<String, String> {
    let cfg = state.config.lock().unwrap().clone();
    if cfg.relay_topic.trim().is_empty() {
        return Err(i18n::resolve(&cfg.language).export_need_topic().into());
    }
    // Single-quote values for the generated bash (Rust's {:?} is NOT shell-safe:
    // it wouldn't escape $, backticks, or `!` in a double-quoted bash context).
    let sh_quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let token_line = if cfg.relay_token.trim().is_empty() {
        String::new()
    } else {
        format!("export CSM_RELAY_TOKEN={}", sh_quote(&cfg.relay_token))
    };
    // Raw template (literal bash) + placeholder substitution — avoids the {{ }}
    // escaping pitfalls of format! for a script this size.
    let template = r#"#!/usr/bin/env bash
# cli-session-monitor — remote agent launcher (generated by the desktop app).
#
# Usage:
#   bash remote-agent.sh                      run the relay agent (relays ALL sessions)
#   bash remote-agent.sh --include-dir DIR    relay ONLY sessions under DIR (repeatable;
#                                             subdirs count). Same as: CSM_WATCH_DIRS=DIR ...
#   bash remote-agent.sh --install-claude     install Claude Code hooks here, then run
#   bash remote-agent.sh --uninstall          remove the Claude hooks installed here, then exit
#
# Needs two small binaries on THIS host: csm-agent and session-reporter. On
# x86_64 Linux this script AUTO-DOWNLOADS them from the GitHub release on first
# run (needs curl or wget; static, no Rust). Alternatives: set CSM_AGENT_BIN /
# CSM_REPORTER_BIN, drop the binaries next to this script, or run inside a repo
# checkout with cargo (it builds them).
#
# Monitors BOTH CLIs and relays session status to your ntfy topic (the desktop app
# subscribes to it). Codex is automatic (its rollout files); Claude Code needs its
# hooks installed on THIS host — run --install-claude (and --uninstall to remove).
# The agent itself installs nothing persistent: Ctrl-C to stop, delete this file.
set -euo pipefail
export CSM_RELAY_URL=@@URL@@
export CSM_RELAY_TOPIC=@@TOPIC@@
@@TOKEN@@

# Parse args: modes (--install-claude / --uninstall) and the directory whitelist
# (--include-dir DIR, repeatable). --include-dir builds CSM_WATCH_DIRS, so the agent
# relays ONLY sessions whose working dir is inside one of those dirs.
MODE="run"
INCLUDE_DIRS=""
while [ $# -gt 0 ]; do
  case "$1" in
    --install-claude) MODE="install" ;;
    --uninstall)      MODE="uninstall" ;;
    --include-dir)
      shift
      [ $# -gt 0 ] || { echo "--include-dir needs a path" >&2; exit 2; }
      INCLUDE_DIRS="${INCLUDE_DIRS:+$INCLUDE_DIRS:}$1" ;;
    --include-dir=*)  INCLUDE_DIRS="${INCLUDE_DIRS:+$INCLUDE_DIRS:}${1#*=}" ;;
    -h|--help)
      echo "Usage: bash remote-agent.sh [--include-dir DIR ...] [--install-claude | --uninstall]" >&2
      echo "  --include-dir DIR   relay ONLY sessions under DIR (repeatable; subdirs count)" >&2
      echo "  --install-claude    install Claude Code hooks here, then run" >&2
      echo "  --uninstall         remove the Claude hooks, then exit" >&2
      exit 0 ;;
    *) echo "unknown argument: $1 (try --help)" >&2; exit 2 ;;
  esac
  shift
done
# --include-dir wins; else honor an externally-set CSM_WATCH_DIRS; empty = relay all.
if [ -n "$INCLUDE_DIRS" ]; then
  export CSM_WATCH_DIRS="$INCLUDE_DIRS"
else
  export CSM_WATCH_DIRS="${CSM_WATCH_DIRS:-}"
fi

# Prebuilt static binaries live in the GitHub release; auto-fetched on first run
# (x86_64 Linux only — the prebuilt target is x86_64-musl). Override the repo with
# CSM_RELEASE_REPO if you forked.
RELEASE_REPO="${CSM_RELEASE_REPO:-reearner/cli-session-monitor}"
RELEASE_TARBALL="csm-remote-x86_64-linux.tar.gz"
_fetched=0
fetch_release() {
  [ "$_fetched" = 1 ] && return 1            # only try once per run
  _fetched=1
  [ "$(uname -m)" = "x86_64" ] || return 1   # prebuilt is x86_64 only
  local url="https://github.com/$RELEASE_REPO/releases/latest/download/$RELEASE_TARBALL"
  echo "Fetching prebuilt agent: $url" >&2
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$RELEASE_TARBALL" || return 1
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$RELEASE_TARBALL" "$url" || return 1
  else
    return 1                                  # no downloader available
  fi
  tar -xzf "$RELEASE_TARBALL" || return 1
  chmod +x ./csm-agent ./session-reporter 2>/dev/null || true
  return 0
}

# Locate a binary: $3 override, PATH, ./name, auto-downloaded release, or (in a
# repo w/ cargo) build it. The cargo branch only succeeds if the build produced
# the binary (so a failed build reports "not found" with guidance, not a bogus
# path later).
locate() {
  local bin="$3"
  if [ -z "$bin" ]; then
    if command -v "$1" >/dev/null 2>&1; then bin="$(command -v "$1")";
    elif [ -x "./$1" ]; then bin="./$1";
    elif fetch_release && [ -x "./$1" ]; then bin="./$1";
    elif command -v cargo >/dev/null 2>&1 && [ -f Cargo.toml ]; then
      if cargo build --release -p "$2" >&2 && [ -x "./target/release/$1" ]; then
        bin="./target/release/$1";
      fi
    fi
  fi
  printf '%s' "$bin"
}

if [ "$MODE" = "install" ] || [ "$MODE" = "uninstall" ]; then
  REPORTER="$(locate session-reporter csm-reporter "${CSM_REPORTER_BIN:-}")"
  if [ -z "$REPORTER" ]; then
    echo "session-reporter not found (auto-download needs curl/wget on x86_64 Linux)." >&2
    echo "Manually: grab csm-remote-x86_64-linux.tar.gz from the release and extract here," >&2
    echo "or set CSM_REPORTER_BIN=/path, or run in a repo checkout with Rust/cargo." >&2
    exit 1
  fi
fi

case "$MODE" in
  uninstall)
    "$REPORTER" --uninstall
    echo "Claude hooks removed. Stop a running agent with Ctrl-C; delete this script to finish."
    exit 0
    ;;
  install)
    "$REPORTER" --install
    ;;
esac

AGENT="$(locate csm-agent csm-agent "${CSM_AGENT_BIN:-}")"
if [ -z "$AGENT" ]; then
  echo "csm-agent not found (auto-download needs curl/wget on x86_64 Linux)." >&2
  echo "Manually: grab csm-remote-x86_64-linux.tar.gz from the release and extract here," >&2
  echo "or set CSM_AGENT_BIN=/path, or run in a repo checkout with Rust/cargo." >&2
  exit 1
fi
echo "csm-agent -> $CSM_RELAY_URL/$CSM_RELAY_TOPIC"
exec "$AGENT"
"#;
    let script = template
        .replace("@@URL@@", &sh_quote(&cfg.relay_url))
        .replace("@@TOPIC@@", &sh_quote(&cfg.relay_topic))
        .replace("@@TOKEN@@", &token_line);
    let path = csm_core::paths::data_dir().join("remote-agent.sh");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Whether a relay topic is configured (`subscribed`) and whether its
/// subscription stream is currently open (`connected`) — for the Settings tag.
#[derive(serde::Serialize)]
struct RelayStatus {
    subscribed: bool,
    connected: bool,
}

#[tauri::command]
fn relay_status(state: State<AppState>) -> RelayStatus {
    let subscribed = !state.config.lock().unwrap().relay_topic.trim().is_empty();
    let connected = state
        .relay_connected
        .load(std::sync::atomic::Ordering::SeqCst);
    RelayStatus {
        subscribed,
        connected,
    }
}

/// Fire a test desktop notification so the user can verify notifications work.
#[tauri::command]
fn test_notification(app: AppHandle, state: State<AppState>) {
    let cfg = state.config.lock().unwrap().clone();
    notify::test(&app, &cfg);
}

#[tauri::command]
fn uninstall_integration(target: String) -> Result<InstallOutcome, String> {
    match target.as_str() {
        "claude" => installer::claude::uninstall(&csm_core::paths::claude_settings_path()),
        "codex" => installer::codex::uninstall(&csm_core::paths::codex_config_path()),
        other => return Err(format!("unknown target: {other}")),
    }
    .map_err(|e| e.to_string())
}

// ---------------- background wiring ----------------

/// Consume events from the file bus, drive the state machine, push snapshots to
/// the UI, and fire completion notifications.
fn spawn_event_loop(app: AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel::<Event>();

    // Demo mode: synthetic sessions only — skip the real sources, relay, and
    // discovery so a recording shows the UI with no real data. The consumer
    // thread below still runs (it drives the state machine + UI from `rx`).
    if demo::enabled() {
        thread::spawn(move || demo::run(tx));
        spawn_consumer(app, rx);
        return;
    }

    let tx_codex = tx.clone();

    // Keep a sender so the relay subscription can be (re)started live, then start
    // it from the current config (no-op if no topic set).
    {
        let cfg = {
            let state = app.state::<AppState>();
            *state.relay_tx.lock().unwrap() = Some(tx.clone());
            let cfg = state.config.lock().unwrap().clone();
            cfg
        };
        start_relay(&app, &cfg);
    }

    // Source: local file bus (Claude hooks + Codex notify, if installed).
    thread::spawn(move || {
        FsWatchSource::new(FsWatchSource::default_dir()).run(tx);
    });
    // Source: Codex rollout tailer (running / waiting-approval / done w/ timing).
    thread::spawn(move || {
        CodexRolloutSource::new(CodexRolloutSource::default_dir()).run(tx_codex);
    });
    // Discovery: surface on-disk sessions so they can be jumped to without typing
    // first. Status is judged by the session itself: a discovered session shows as
    // "waiting" when an actual codex/claude process is running in its launch dir
    // (the CLI is open), else idle. (Jump uses window matching, separately.) Remote
    // sessions can't be process-checked locally, so they stay idle here.
    {
        let app_disc = app.clone();
        let local = csm_core::paths::host_name().to_lowercase();
        thread::spawn(move || loop {
            let cli_dirs = running_cli_dirs();
            // User-configurable retention: keep surfacing local sessions used in
            // the last N days (recurring sessions stay around).
            let window_secs = {
                let days = app_disc
                    .state::<AppState>()
                    .config
                    .lock()
                    .unwrap()
                    .discover_window_days
                    .max(1) as u64;
                days * 24 * 3600
            };
            let raw = csm_watch::discover_sessions(window_secs);
            // Diagnostics only when CSM_DEBUG is on (also skips the window scan).
            if applog::enabled() {
                let titles = all_visible_titles();
                applog::line(&format!(
                    "[discover] sessions={} live_cli_procs={} windows={}",
                    raw.len(),
                    cli_dirs.len(),
                    titles.len()
                ));
                for t in titles.iter().filter(|t| {
                    let l = t.to_lowercase();
                    l.contains("cursor")
                        || l.contains("code")
                        || l.contains(":\\")
                        || l.contains('/')
                }) {
                    applog::line(&format!("[discover]   win: {t}"));
                }
            }
            let items: Vec<(Event, bool)> = raw
                .into_iter()
                .map(|ev| {
                    let is_local = ev.host.to_lowercase() == local;
                    let dir = normalize_dir(&ev.cwd);
                    let active =
                        is_local && cli_dirs.iter().any(|(s, d)| *s == ev.source && *d == dir);
                    if applog::enabled() {
                        applog::line(&format!(
                            "[discover]   {:?} {} active={}",
                            ev.source, dir, active
                        ));
                    }
                    (ev, active)
                })
                .collect();

            // Collapse parent/child duplicates of the SAME session: when a live
            // CLI process anchors a dir (active), drop inactive discovered
            // sessions of the same source in an ancestor/descendant dir — that's
            // the same session seen at a cd'd cwd (e.g. Cursor opened the parent,
            // Claude cd'd into a subdir), not a second one. Unrelated dirs untouched.
            let anchors: Vec<(CliSource, String)> = items
                .iter()
                .filter(|(_, a)| *a)
                .map(|(e, _)| (e.source, normalize_dir(&e.cwd)))
                .collect();
            let items: Vec<(Event, bool)> = items
                .into_iter()
                .filter(|(e, a)| {
                    if *a {
                        return true;
                    }
                    let d = normalize_dir(&e.cwd);
                    let overlapped = anchors.iter().any(|(s, ad)| {
                        *s == e.source
                            && (d == *ad
                                || d.starts_with(&format!("{ad}\\"))
                                || ad.starts_with(&format!("{d}\\")))
                    });
                    if overlapped && applog::enabled() {
                        applog::line(&format!("[discover]   drop overlapped idle {d}"));
                    }
                    !overlapped
                })
                .collect();

            let state = app_disc.state::<AppState>();
            let (changed, full) = {
                let mut sm = state.sm.lock().unwrap();
                let changed = sm.reconcile_discovered(items);
                (changed, sm.snapshot())
            };
            log_snapshot("state", &full); // every cycle, so the steady state is visible
            if changed {
                let _ = app_disc.emit("sessions:update", &full);
            }
            thread::sleep(Duration::from_secs(DISCOVER_INTERVAL_SECS));
        });
    }

    spawn_consumer(app, rx);
}

/// Log the exact cards the UI will render (CSM_DEBUG only): per session its
/// source, dir, status, and id — `disc:` ids are discovery placeholders, anything
/// else is a real event-driven session. Lets failures be diagnosed from the log.
fn log_snapshot(tag: &str, snap: &[SessionView]) {
    if !applog::enabled() {
        return;
    }
    applog::line(&format!("[cards:{tag}] count={}", snap.len()));
    for v in snap {
        applog::line(&format!(
            "[cards:{tag}]   {:?} {} status={:?} id={}",
            v.source, v.cwd, v.status, v.key.session_id
        ));
    }
}

/// Drive the state machine from the event channel, push snapshots to the UI, and
/// fire flash + completion notifications. Shared by the real and demo paths.
fn spawn_consumer(app: AppHandle, rx: std::sync::mpsc::Receiver<Event>) {
    // In demo mode show the in-app flash but don't raise OS toasts (no spamming
    // the notification center while recording).
    let demo = demo::enabled();
    let local_host = csm_core::paths::host_name();
    // Per-host clock offset (local_now - remote_ts), so a remote machine's clock
    // being a few seconds off doesn't make the live timer read 0:00 or balloon.
    let mut host_offset: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    thread::spawn(move || {
        for mut ev in rx {
            // Align a REMOTE event's timestamps to the local clock. Calibrate the
            // offset only from near-real-time events (an `since=` replay of old
            // events would otherwise poison it); applying a stable per-host offset
            // preserves the relative spacing between a host's events.
            if !ev.host.is_empty() && ev.host != local_host {
                let skew = now_ms() - ev.ts;
                if skew.abs() < NOTIFY_GRACE_MS {
                    host_offset.insert(ev.host.clone(), skew);
                }
                ev.ts += host_offset.get(&ev.host).copied().unwrap_or(0);
            }
            // Suppress notifications/flash for replayed/old events (e.g. ntfy
            // `since=` catch-up on connect) — still show the card, just don't nag.
            let fresh = now_ms() - ev.ts < NOTIFY_GRACE_MS;
            let state = app.state::<AppState>();
            let (effects, snapshot) = {
                let mut sm = state.sm.lock().unwrap();
                let effects = sm.apply(ev);
                (effects, sm.snapshot())
            };
            let _ = app.emit("sessions:update", &snapshot);
            log_snapshot("event", &snapshot);
            if fresh && !effects.is_empty() {
                let cfg = state.config.lock().unwrap().clone();
                for e in &effects {
                    match e {
                        Effect::Completed(key) => {
                            // Turn finished ("your turn to type") -> green "done"
                            // flash, distinct from the amber "awaiting your choice".
                            let _ = app.emit("session:flash", FlashPayload { kind: "done", key });
                            if !demo {
                                notify::on_completed(&app, key, &snapshot, &cfg);
                            }
                        }
                        Effect::AwaitingInput(key) => {
                            // Blocked needing a decision/approval -> amber "waiting".
                            let _ =
                                app.emit("session:flash", FlashPayload { kind: "waiting", key });
                            if !demo {
                                notify::on_awaiting_input(&app, key, &snapshot, &cfg);
                            }
                        }
                    }
                }
            }
        }
    });
}

/// Periodically advance `done` -> `idle` and re-emit when something changed.
fn spawn_idle_ticker(app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1));
        let state = app.state::<AppState>();
        let snap = {
            let mut sm = state.sm.lock().unwrap();
            if sm.tick(now_ms()) {
                Some(sm.snapshot())
            } else {
                None
            }
        };
        if let Some(s) = snap {
            let _ = app.emit("sessions:update", &s);
        }
    });
}

/// Apply desktop-resident window preferences (all default OFF, user-toggleable):
/// taskbar visibility, desktop-pin vs always-on-top, left docking, and autostart.
fn apply_window_prefs(app: &AppHandle, cfg: &Config) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_skip_taskbar(cfg.skip_taskbar);
        // Lightweight (floating-ball) mode => tool window: out of taskbar AND the
        // Alt-Tab switcher, so it behaves like a desktop orb, not an app.
        set_tool_window(&win, cfg.lightweight);

        if cfg.desktop_pinned {
            let _ = win.set_always_on_top(false);
            let _ = win.set_always_on_bottom(true);
        } else {
            let _ = win.set_always_on_bottom(false);
            let _ = win.set_always_on_top(cfg.always_on_top);
        }

        if cfg.dock_left {
            if let Ok(Some(monitor)) = win.current_monitor() {
                let msize = monitor.size();
                let mpos = monitor.position();
                let wsize = win
                    .outer_size()
                    .unwrap_or(tauri::PhysicalSize::new(360, 540));
                let y = mpos.y + (((msize.height as i32) - (wsize.height as i32)) / 2).max(0);
                let _ = win.set_position(tauri::PhysicalPosition::new(mpos.x, y));
            }
        }
    }

    let autolaunch = app.autolaunch();
    if cfg.autostart {
        let _ = autolaunch.enable();
    } else {
        let _ = autolaunch.disable();
    }
}

/// Toggle the tool-window extended style so the window is hidden from the
/// taskbar and the Alt-Tab switcher (floating-ball behavior). Windows only.
#[cfg(windows)]
fn set_tool_window(window: &tauri::WebviewWindow, enable: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, SWP_FRAMECHANGED,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    let hwnd = match window.hwnd() {
        Ok(h) => HWND(h.0 as _),
        Err(_) => return,
    };
    unsafe {
        let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if enable {
            ex |= WS_EX_TOOLWINDOW.0 as isize;
            ex &= !(WS_EX_APPWINDOW.0 as isize);
        } else {
            ex &= !(WS_EX_TOOLWINDOW.0 as isize);
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
    }
}

#[cfg(not(windows))]
fn set_tool_window(_window: &tauri::WebviewWindow, _enable: bool) {}

/// Toggle edge-drag resizing (WS_THICKFRAME) directly. Tauri's `setResizable`
/// doesn't take effect on this borderless/transparent window, so the full panel
/// is made resizable and the ball/bar fixed by flipping the style here.
#[cfg(windows)]
#[tauri::command]
fn set_resizable(window: tauri::WebviewWindow, enable: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_THICKFRAME,
    };
    let hwnd = match window.hwnd() {
        Ok(h) => HWND(h.0 as _),
        Err(_) => return,
    };
    unsafe {
        let mut style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        if enable {
            style |= WS_THICKFRAME.0 as isize;
        } else {
            style &= !(WS_THICKFRAME.0 as isize);
        }
        SetWindowLongPtrW(hwnd, GWL_STYLE, style);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );
    }
}

#[cfg(not(windows))]
#[tauri::command]
fn set_resizable(_window: tauri::WebviewWindow, _enable: bool) {}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let lang = i18n::resolve(&app.state::<AppState>().config.lock().unwrap().language);
    let show = MenuItem::with_id(app, "toggle", lang.tray_toggle(), true, None::<&str>)?;
    let reset = MenuItem::with_id(app, "reset", lang.tray_reset(), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", lang.tray_quit(), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &reset, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .tooltip("CLI Session Monitor")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "toggle" => toggle_main_window(app),
            "reset" => {
                // Pull the widget back onto a visible edge if it got lost
                // (off-screen, behind something, docked somewhere unnoticed).
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.unminimize();
                    let _ = win.set_always_on_top(true);
                }
                let _ = app.emit("csm:reset-pos", ());
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

fn toggle_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if win.is_visible().unwrap_or(true) {
            let _ = win.hide();
        } else {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

fn main() {
    let config = Config::load_from(Config::default_path());
    let sm = StateMachine::new(config.idle_threshold_secs);
    let startup_config = config.clone();

    tauri::Builder::default()
        // single-instance MUST be the first plugin: a second launch just focuses
        // the running window instead of starting another process.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState {
            sm: Mutex::new(sm),
            config: Mutex::new(config),
            relay_tx: Mutex::new(None),
            relay_stop: Mutex::new(None),
            relay_connected: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            editor_map: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_config,
            set_config,
            integration_status,
            install_integration,
            uninstall_integration,
            export_agent_script,
            relay_status,
            test_notification,
            save_window_pos,
            save_window_size,
            set_session_name,
            set_session_cmd,
            set_resizable,
            set_window_bounds,
            local_host,
            open_session,
            session_window_titles,
            optimize_editor_jump,
            revert_editor_jump,
            editor_jump_status,
            dismiss_session,
            active_window_cards
        ])
        .on_window_event(|window, event| {
            // Closing the window hides to tray instead of quitting (Req 7.3).
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            // In-app auto-update (checked from the frontend on launch). Non-fatal:
            // if it can't init, the app still runs.
            let _ = app
                .handle()
                .plugin(tauri_plugin_updater::Builder::new().build());
            applog::line(&format!(
                "[start] host={} idle_secs={} relay_topic={}",
                csm_core::paths::host_name(),
                startup_config.idle_threshold_secs,
                if startup_config.relay_topic.trim().is_empty() {
                    "(none)"
                } else {
                    "(set)"
                }
            ));
            apply_window_prefs(&handle, &startup_config);
            build_tray(&handle)?;
            spawn_event_loop(handle.clone());
            spawn_idle_ticker(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::Win32::Foundation::HWND;

    // best_editor_window ignores the HWND value (returns the matched index), so a
    // dummy handle is fine for testing the title/cwd matching.
    fn h(n: isize) -> HWND {
        HWND(n as _)
    }

    #[test]
    fn matches_chinese_folder_path() {
        // Window title carrying a full Chinese path (as "Optimize jump" produces).
        let wins = vec![
            (h(1), "其它项目 - Cursor".to_string()),
            (h(2), "D:\\代码\\我的项目 - Cursor".to_string()),
        ];
        assert_eq!(
            best_editor_window("D:\\代码\\我的项目", "", &wins, None),
            Some(1)
        );
    }

    #[test]
    fn matches_chinese_basename_in_default_title() {
        // VS Code's default "file - folder - app" title; the suffix matcher should
        // still find a Chinese folder basename.
        let wins = vec![(h(7), "main.rs - 我的项目 - Visual Studio Code".to_string())];
        assert_eq!(
            best_editor_window("D:\\代码\\我的项目", "", &wins, None),
            Some(0)
        );
    }

    #[test]
    fn chinese_remote_path_matches_via_host() {
        // Remote-SSH window: Chinese folder + [SSH: host]; host disambiguates.
        let wins = vec![(h(3), "我的项目 [SSH: build-server] - Cursor".to_string())];
        assert_eq!(
            best_editor_window("/srv/我的项目", "build-server", &wins, None),
            Some(0)
        );
    }

    #[test]
    fn remote_session_does_not_jump_to_a_same_named_local_window() {
        // A session on another host (e.g. Codex inside a VM) whose folder tail
        // happens to match a LOCAL window's folder must NOT focus that local
        // window — there is no Remote-SSH window for its host, so the answer is
        // "no match", not the wrong window.
        let wins = vec![(h(1), "D:\\work\\desktop - Cursor".to_string())];
        assert_eq!(
            best_editor_window("/home/tsf/desktop", "tsf-virtual-machine", &wins, None),
            None
        );
    }

    #[test]
    fn same_folder_in_two_editors_is_ambiguous_without_a_hint() {
        // Identical Chinese folder open in both Cursor and VS Code -> tie -> None.
        let wins = vec![
            (h(1), "D:\\代码\\我的项目 - Visual Studio Code".to_string()),
            (h(2), "D:\\代码\\我的项目 - Cursor [Administrator]".to_string()),
        ];
        assert_eq!(best_editor_window("D:\\代码\\我的项目", "", &wins, None), None);
    }

    #[test]
    fn editor_hint_breaks_the_two_editor_tie() {
        // Same tie, but knowing the CLI runs under Cursor picks the Cursor window;
        // knowing it runs under VS Code picks the other.
        let wins = vec![
            (h(1), "D:\\代码\\我的项目 - Visual Studio Code".to_string()),
            (h(2), "D:\\代码\\我的项目 - Cursor [Administrator]".to_string()),
        ];
        assert_eq!(
            best_editor_window("D:\\代码\\我的项目", "", &wins, Some("cursor")),
            Some(1)
        );
        assert_eq!(
            best_editor_window("D:\\代码\\我的项目", "", &wins, Some("code")),
            Some(0)
        );
    }
}
