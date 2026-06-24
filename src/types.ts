// Mirrors the Rust types serialized across the Tauri IPC boundary.
// Keep in sync with csm_core::{Source} and src-tauri state::{SessionView,SessionStatus}
// and config::Config and installer::InstallOutcome.

export type Source = "claude-code" | "codex";
export type Status = "running" | "waiting" | "done" | "idle";

export interface SessionKey {
  source: Source;
  host: string;
  session_id: string;
}

export interface SessionView {
  key: SessionKey;
  source: Source;
  host: string;
  cwd: string;
  status: Status;
  run_started_at: number | null;
  run_ended_at: number | null;
  last_duration_ms: number | null;
  timing_reliable: boolean;
}

export interface Config {
  notifications: boolean;
  sound: boolean;
  idle_threshold_secs: number;
  always_on_top: boolean;
  autostart: boolean;
  dock_left: boolean;
  desktop_pinned: boolean;
  skip_taskbar: boolean;
  lightweight: boolean;
  relay_url: string;
  relay_topic: string;
  relay_token: string;
  win_x: number | null;
  win_y: number | null;
  language: string;
  panel_w: number;
  panel_h: number;
  onboarded: boolean;
}

export interface InstallOutcome {
  config_path: string;
  changed: boolean;
  installed: boolean;
  conflict: string | null;
  backup_path: string | null;
  summary: string;
}

export type IntegrationTarget = "claude" | "codex";

export interface IntegrationStatus {
  claude: InstallOutcome;
  codex: InstallOutcome;
}

/** Stable string id for a session, for DOM keying. */
export function keyId(k: SessionKey): string {
  return `${k.source}::${k.host}::${k.session_id}`;
}

export function sourceLabel(s: Source): string {
  return s === "claude-code" ? "Claude Code" : "Codex";
}
