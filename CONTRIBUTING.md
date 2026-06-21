# Contributing

Thanks for your interest! This is an early-stage project — issues and PRs welcome.

## Project layout

```
crates/csm-core      shared types (Event/EventKind/Source/SessionKey), paths      (cross-platform)
crates/csm-reporter  the tiny binary Claude Code hooks invoke; normalizes -> file bus (cross-platform)
crates/csm-watch     event Sources: codex rollout tailer, file-bus watch, ntfy, discovery (cross-platform)
crates/csm-agent     remote-side binary: watches + publishes to an ntfy topic        (cross-platform)
src-tauri            the desktop app (state machine, tray, window, IPC commands)     (Windows GUI)
src/                 frontend (vanilla TS + Vite)
```

The core data path is decoupled by a **local file bus** (`~/.cli-session-monitor/events/`).
See `docs/DESIGN.md` for the architecture and `docs/REMOTE-DESIGN.md` for remote.

## Build & test

```bash
cargo test                 # pure-logic crates + app backend (no CLI/GUI needed)
npm install && npm run typecheck
npm run tauri dev          # run the app (Windows)
npm run tauri build        # release build + installer (Windows)
```

The cross-platform crates build on Linux/macOS; the **desktop app, window-jump
and process detection are Windows-only** (other platforms use no-op stubs).

## Ground rules

- **`session-reporter` must never block or break the calling CLI** — bounded
  stdin read, swallow all errors, always exit 0. This is the project's top
  invariant.
- **Only metadata** is read/transmitted — never conversation content.
- Keep the state machine **pure** (no IO / no clock access; `tick(now)` takes the
  clock as a parameter) so it stays deterministically testable.
- Config edits to other tools (`~/.claude/settings.json`, editor settings) must be
  append-only, backup-first, idempotent, and reversible.

## Debugging

Set `CSM_DEBUG=1` before launching to write diagnostics (startup, discovery +
visible window titles, jump decisions) to `~/.cli-session-monitor/csm.log`. It's
off by default — normal runs write no log and skip the extra window scan.

`CSM_DEMO=1` runs the app on synthetic sessions (no real sources/relay/discovery,
a fake hostname, synthesized window titles, OS notifications suppressed) — for
screenshots/recordings and a zero-setup first look. See `src-tauri/src/demo.rs`.

## Before opening a PR

- `cargo test` and `npm run typecheck` pass.
- `cargo fmt --all` and (ideally) `cargo clippy` are clean.
- Add/adjust tests for logic changes; describe user-facing behavior in the PR.
