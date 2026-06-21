# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Project scaffold: cargo workspace (`csm-core`, `csm-reporter`, `src-tauri`), MIT license, CI placeholder.
- `csm-core`: unified event schema (`Event`, `EventKind`, `Source`, `SessionKey`) with `schema` version field and `host` for remote/cross-device support.
- `session-reporter`: normalizes Claude Code hook (stdin) and Codex notify (argv/stdin) events into the local file bus via an atomic-write `FileSink`; never blocks the calling CLI (always exits 0).
- App backend engine (in `src-tauri`, pre-GUI): `Config` (load/save with safe defaults), the session `StateMachine` (`running`→`done`→`idle`, `session_end` removal, unreliable-timing degradation for Codex), and `FsWatchSource` (startup drain + watch + corrupt-file skip). Fully unit/integration tested without any CLI.

### Added (continued)
- `csm_core::paths`: centralized cross-platform path/hostname resolution (single platform-branch point); `host_name` now uses the OS hostname via `gethostname` so it works under non-interactive hooks on Linux. Reporter/app delegate to it.
- Installer (`src-tauri/src/installer`): one-click Claude Code (`settings.json` hooks) and Codex (`config.toml` `notify`) integration — append-only, backup-first, idempotent, reversible, with Codex foreign-`notify` conflict detection (never overwrites) and abort-on-parse-error. `toml_edit` preserves the user's Codex comments/formatting.
- Tauri v2 desktop app (`src-tauri`): wires the file-bus `Source` → `StateMachine` → `sessions:update` events and completion notifications (`tauri-plugin-notification`); system tray (show/hide, quit); frameless always-on-top window that hides to tray on close; IPC commands `get_snapshot`/`get_config`/`set_config`/`integration_status`/`install_integration`/`uninstall_integration`. Builds and runs on Windows via a portable MSVC toolchain installed entirely on D:. App icons generated.
- Desktop-resident options (all default OFF, toggleable in Settings → 桌面常驻): autostart on login (`tauri-plugin-autostart`), dock to the left screen edge, desktop-pinned mode (always-on-bottom instead of always-on-top), and hide-from-taskbar (tray only). Applied on startup and live on change.
- Frontend (Vanilla TS + Vite, in `src/`): compact dark widget — session cards with status dot, source/host/cwd, a live per-second timer computed locally from `run_started_at`, an "估算" badge when timing is unreliable (Codex); a settings/integration panel with notification & always-on-top toggles and one-click install/uninstall buttons that surface the backend's change summary. Typed IPC layer (`ipc.ts`) defines the command/event contract. `tsc --noEmit` clean. (Runtime wiring needs the Tauri backend, tasks 12–13.)

### Added (Codex first-class via rollout watching)
- `CodexRolloutSource` (`src-tauri/src/source/codex_rollout.rs`): tails Codex's per-session rollout JSONL (`~/.codex/sessions/**/rollout-*.jsonl`) to give Codex parity with Claude — `task_started` → running (with a real live timer), `task_complete`/`turn_aborted` → done with accurate duration, and `exec_approval_request`/`apply_patch_approval_request` → a new **Waiting** status + "等待你的操作" notification (the agent is blocked needing your approval). Reads only metadata (event type, ids, timestamps); never conversation content. The user runs Codex normally; no config changes needed. This resolves Codex's previous "no running state / estimated timing" limitation (notify only fires at turn end).
- New `EventKind::WaitingInput`, `SessionStatus::Waiting` (ranked first in the UI), and `Effect::AwaitingInput`; `RunStart` now keeps the timer on resume/continuation (so mid-turn commands and post-approval resumption don't reset elapsed time). Frontend shows a distinct amber "等待审批" card.

### Added (floating ball, remote monitoring & polish)
- Lightweight "floating ball" mode: the widget collapses to a small orb that, after 1s idle, docks to the nearest screen edge as a thin bar; hovering pops it back; hidden from the taskbar **and** Alt-Tab (tool window). Drag-to-edge snaps and re-orients (vertical on left/right, horizontal on top/bottom) without first morphing back to a ball.
- Docked edge bar shows colour-coded counts at a glance: ▶ running (blue), ! awaiting-input (amber), ✓ done (green); each hides at zero.
- Expanding the panel clamps it on-screen (a bottom ball expands upward, a right one leftward) and uses a single atomic `SetWindowPos` (position+size in one op) to avoid the grow-then-jump flicker. Collapsing returns the ball to its pre-expand position; the docked position persists across restarts (`win_x`/`win_y`).
- Acknowledge-on-collapse: after opening the full panel and collapsing back, the orb keeps its amber waiting colour but stops the continuous pulse; a fresh completion/waiting flash re-arms it.
- Remote monitoring without SSH via an ntfy relay: `csm-watch` crate (extracted shared `Source`s + `NtfySource` publish/subscribe), a `csm-agent` binary for the remote host (watches Codex rollouts + Claude file-bus, publishes to a topic), and app-side subscription (`relay_url`/`relay_topic`/`relay_token`). Settings can export a ready-to-run `remote-agent.sh` with the topic baked in. Only metadata crosses the relay; phone notifications are just another subscriber. Verified end-to-end through real ntfy.sh.

### Added (sessions, jump & status)
- Session discovery: surfaces already-open Claude/Codex sessions found on disk (so they show before you type), rescanned every 30s and de-duped to one card per directory. Status is judged by the live CLI **process** — a session shows "waiting for input" only when an actual `codex.exe`/`claude.exe` is running in its directory, else idle.
- Click a card to **jump to its editor window**: matches the Cursor/VS Code window by title (folder names + host for Remote-SSH), focuses it without shrinking a maximized window; remembers which editor each directory uses and reopens exactly that one (no try-everything chain). Cards show the launch dir + the matched window.
- One-click "Optimize jump" writes a `window.title` (with the folder path) into VS Code/Cursor settings so same-named folders are distinguishable; shows configured status; reversible.
- Finished turns are presented as "waiting for input" (consistent with Claude); the done→idle retention is configurable (Settings → minutes).

### Fixed
- Restart/collapse positioning: the widget restores its last docked position on launch (no longer snaps to the top-left), and collapsing the expanded panel returns to the bar in one step with no flash at the panel's corner.
- Idle decay is timed from when the app first observes a session done (local clock), so a finished **remote** session no longer jumps straight to idle under clock skew.
- Multi-window jump accuracy: the folder name is the discriminator (host only breaks ties); ambiguous matches don't jump rather than focusing the wrong window.

### Fixed
- Claude "waiting for input" detection: Claude pausing mid-turn (permission prompt / clarifying question) fires the `Notification` hook, not `Stop`, so the widget previously stayed on "running". Now `Notification → waiting_input`, and `PreToolUse → run_start` clears a stale "waiting" once Claude resumes running tools (a Notification has no matching resume event within the same turn). Installer now writes these hooks too.
- Codex completion now fires only when Codex hands the floor back (`task_complete`), not prematurely during long-running commands; a 2s poll fallback covers Windows file-watch dropping appends; pre-startup sessions get their `cwd` from the rollout head.

### Verified
- Codex notify capability probe (Codex CLI 0.130.0): the `agent-turn-complete` event carries a stable `thread-id` (session) plus per-turn `turn-id` and `cwd`; there is no "turn started" signal (so Codex timing degrades, as designed). Fixed the Codex adapter to key the session on `thread-id`, locked it with a real-payload test, and confirmed end-to-end `codex exec` → notify → `session-reporter` → file-bus event with no conversation content leaking.
