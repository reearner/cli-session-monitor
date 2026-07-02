# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.6] - 2026-07-01

### Added
- **Per-card resume command.** Right-click a card's ⧉ button to edit and remember the exact command to relaunch that session — so flags like `--yolo` / `--dangerously-skip-permissions` that the default command drops are preserved. Stored by session id, persists across restarts. A card with a custom command marks its ⧉ in the accent color.
- **Grey/idle cards are copyable too.** Discovered cards (folder only, no session id) now have the ⧉ button; it copies a "continue the most recent session in this folder" command (`claude --continue` / `codex resume --last`).
- **Completion notifications name the session.** The toast title now leads with the card's name (your custom rename if set, else the project folder) — e.g. `academic_paper · Claude Code finished` — so you can tell which session pinged you, not just its path.
- **Config is backed up before meaningful saves.** Settings/name/command changes first archive the previous `config.json` to `~/.cli-session-monitor/backups/` (newest 10 kept), so a bad write can't silently lose your data.

### Changed
- **Longer default retention:** a finished (green) card now stays **1 hour** before dimming to idle (was 2 min), and idle cards are kept **3 days** (was 1). Both still adjustable in Settings.

### Fixed
- **Card names & remembered commands are no longer wiped by saving a setting.** `set_config` replaced the whole config with the settings panel's (stale) snapshot, blanking `session_names`/`session_cmds`; these fields (and window position/size) are now owned by the backend and preserved.
- **Editing a name/command is no longer interrupted by a flicker.** A background snapshot push could rebuild the card list and destroy the open input mid-typing; re-renders now pause while an inline editor is open.
- **No more random flicker ("screen twitch").** The panel rebuilt the whole card list on every backend push — including changes to fields it doesn't display (e.g. a running session's per-tool-call events) — flashing the transparent window. It now re-renders only when the visible content actually changes.

## [0.1.5] - 2026-06-28

### Added
- **Rename a card.** Each session card has a ✎ button to give it a custom name; the name is stored by session id, so it persists across restarts and when you resume the same conversation (`claude --resume <id>` / `codex resume <id>`) — making several sessions easy to tell apart. You type the name; no conversation content is read (the metadata-only promise holds).

### Changed
- **Stable card order.** Cards no longer reshuffle when you reopen the panel. They were sorted by most-recent activity, so any event (e.g. a Claude tool call) jumped a card to the top of its group; now each card keeps a fixed slot within its status and only moves when its status actually changes.
- **The docked bar lightens by status.** When collapsed to the thin edge bar, the whole strip now tints to the most-urgent status color (amber = needs you, green = replied, blue = running), so it's noticeable at a glance — complementing the pop-out ball.
- The **"needs your input" flash lasts longer** (~6s, continuously pulsing) than a plain "replied" flash, so a question is harder to miss.

### Fixed
- **Remote sessions no longer jump to the wrong window.** A session on another host (e.g. Codex inside a VM) whose folder name coincided with a local editor window could focus that unrelated local window; it now only matches a Remote-SSH window for its own host, otherwise it doesn't jump.
- The generated **remote-agent.sh auto-downloads** the prebuilt agent binary on first run (x86_64 Linux, via curl/wget) — no manual download needed.

## [0.1.4] - 2026-06-26

### Added
- **In-app auto-update** (Tauri v2 updater): the app checks for a new version on launch and offers a one-click "Install & restart" banner. Update packages are signed with an Ed25519 key and verified against the embedded public key before installing; the launch check fails safe (offline / not-configured simply shows nothing). Releases now sign the artifacts and emit `latest.json` automatically.

### Changed
- **Card redesign — identify the session at a glance.** Each card now leads with the **project name** (the cwd's last path segment) instead of "Claude/Codex"; the parent directory shows as a small ▸ line, and the CLI kind is demoted to a small tag in the footer. Duplicate fields were removed and the layout deduplicated, so it's immediately clear which session a card belongs to.

### Fixed
- Dropping the floating ball / docked bar no longer **blinks**: the dock-to-edge step exposed one empty-window repaint frame (hard `display` toggle of the orb). The orb now hides instantly via opacity and fades back in as the bar over 0.12s, covering that frame (respects `prefers-reduced-motion`).

## [0.1.3] - 2026-06-25

### Added
- **Per-session short id** on each card, plus a **⧉ copy-resume button** that copies the exact resume command (`claude --resume <id>` / `codex resume <id>`) — so you can bring a specific session into the terminal you want when several agents share one editor window (Windows can't focus an individual terminal tab).
- **Configurable retention**: keep idle local-session cards for **1 / 3 / 7 / 14 days** (Settings → "Keep idle sessions for"), so recurring sessions stay around; the session id is retained across restarts for the whole window.
- **Relay status indicator** (Off / Connecting… / Connected ✓) and a **"Send a test notification"** button in Settings — to verify remote monitoring and notifications actually work.
- **First-run onboarding**: Settings opens once on the first ever launch.
- **Frontend tests** (vitest) wired into CI.

### Changed
- The **docked bar counts are color-matched to the cards** (▶ running / ! needs-you / ✓ replied), so a finished turn isn't miscounted as "awaiting input."
- Docs corrected to the four status states (GUIDE en/zh, README); the exported remote-agent script shell-quotes the relay url/topic/token.

### Fixed
- Resizing the full panel from a top/left edge no longer jitters.
- The thin **docked bar can be dragged** again (and clicking it stays reliable).
- **Editor-window jump** disambiguates a folder open in **both Cursor and VS Code** (via the CLI's process ancestry); Chinese/Unicode folder paths verified.
- The **remote live timer** is corrected for clock-skewed remote hosts.
- A blocked "needs your input" pause no longer pops **duplicate notifications**.
- `process_cwd` could read 1 byte past its buffer for an odd-length path.
- Keyboard accessibility (cards focusable + focus rings) and a brief flash when a jump finds no window.
- Performance: the running-process scan is cached (no full re-enumeration on every ~1.2s foreground poll); directory-normalization logic deduplicated into `csm-core`.

## [0.1.2] - 2026-06-22

### Added
- The session card that just changed now **pulses** so you can tell which one it was when you open the panel (color follows status). Clears when you click it or after a few seconds; respects `prefers-reduced-motion` (static highlight).

### Changed
- **Distinguish "replied" from "awaiting your choice."** A finished turn now shows green **Replied / 已回复** (steady dot, green completion flash) — distinct from amber **Needs your input / 等待你确认** (pulsing dot, amber flash) used when the CLI is blocked needing your approval/choice (Claude permission prompt / Codex approval). Previously both looked the same.
- The `session:flash` event carries the session key so the UI can highlight the specific card.

## [0.1.1] - 2026-06-22

### Fixed
- Clicking the docked bar / floating ball is now reliable. The orb was an OS drag region, so any micro-movement while pressing made the system start a window move and the click was lost — worst during a completion flash, when the orb also pops out and repositions. Dragging is now manual (a press that moves past a small threshold starts the drag), so a still press always registers as a click.
- Editor-window match/jump now disambiguates a folder open in **both** Cursor and VS Code by following the CLI's parent-process chain to the editor hosting its integrated terminal (previously an ambiguous tie → "no matching window"). Unicode/Chinese folder paths are covered by regression tests.
- CI: the release checksum step looks in the workspace target dir, so `SHA256SUMS.txt` is published with each release.

### Changed
- Docs: the User Guides explain running the remote agent as a long-running relay (run it detached with `nohup`/`tmux`; one agent covers both Codex and Claude; don't start duplicates), and lead with downloading the prebuilt static Linux binaries (no Rust needed on the server).

## [0.1.0] - 2026-06-21

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
