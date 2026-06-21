// Thin, typed wrappers over the Tauri IPC surface. The command/event names here
// are the contract the Rust backend (src-tauri) must implement.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  SessionView,
  SessionKey,
  Config,
  InstallOutcome,
  IntegrationTarget,
  IntegrationStatus,
} from "./types";

export const getSnapshot = (): Promise<SessionView[]> =>
  invoke<SessionView[]>("get_snapshot");

/** Close (hide) a session card. It reappears if the session acts again. */
export const dismissSession = (key: SessionKey): Promise<void> =>
  invoke("dismiss_session", { key });

/** Which card(s) the foreground window belongs to. `own` = our widget is focused
 *  (keep the previous highlight); otherwise `keys` is the group to highlight. */
export const activeWindowCards = (): Promise<{ own: boolean; keys: SessionKey[] }> =>
  invoke("active_window_cards");

export const getConfig = (): Promise<Config> => invoke<Config>("get_config");

export const setConfig = (config: Config): Promise<Config> =>
  invoke<Config>("set_config", { config });

export const integrationStatus = (): Promise<IntegrationStatus> =>
  invoke<IntegrationStatus>("integration_status");

export const installIntegration = (
  target: IntegrationTarget,
): Promise<InstallOutcome> =>
  invoke<InstallOutcome>("install_integration", { target });

export const uninstallIntegration = (
  target: IntegrationTarget,
): Promise<InstallOutcome> =>
  invoke<InstallOutcome>("uninstall_integration", { target });

/** Write a ready-to-run remote-agent.sh (topic baked in) and return its path. */
export const exportAgentScript = (): Promise<string> =>
  invoke<string>("export_agent_script");

/** Persist the widget position (physical px) for next launch. */
export const saveWindowPos = (x: number, y: number): Promise<void> =>
  invoke("save_window_pos", { x, y });

/** Persist the user-resized full-panel size (logical px) for next launch. */
export const saveWindowSize = (w: number, h: number): Promise<void> =>
  invoke("save_window_size", { w, h });

/** Allow/disallow edge-drag resizing (only the full panel should be resizable). */
export const setResizable = (enable: boolean): Promise<void> =>
  invoke("set_resizable", { enable });

/** Local hostname — used to tell local (openable) sessions from remote ones. */
export const localHost = (): Promise<string> => invoke<string>("local_host");

/** Jump to a session's editor window (focuses an existing Cursor/VS Code window,
 *  incl. Remote-SSH). When `create` is false (e.g. an idle/closed session), only
 *  focus an existing window — never open a new one. */
export const openSession = (
  path: string,
  host: string,
  create: boolean,
): Promise<void> => invoke("open_session", { path, host, create });

/** For each session, the editor window title it maps to (or null). */
export const sessionWindowTitles = (
  sessions: { cwd: string; host: string }[],
): Promise<(string | null)[]> =>
  invoke("session_window_titles", { sessions });

/** Write window.title into VS Code/Cursor settings so jump can disambiguate
 *  same-named folders. Returns a per-editor summary. */
export const optimizeEditorJump = (): Promise<string> =>
  invoke<string>("optimize_editor_jump");

/** Undo optimizeEditorJump (remove the window.title we added). */
export const revertEditorJump = (): Promise<string> =>
  invoke<string>("revert_editor_jump");

/** Whether VS Code/Cursor are configured for accurate jump. */
export const editorJumpStatus = (): Promise<{ configured: boolean; summary: string }> =>
  invoke("editor_jump_status");

/** Atomically set window position + size (one SetWindowPos) to avoid the
 *  grow-then-jump flicker. All values physical px. */
export const setWindowBounds = (
  x: number,
  y: number,
  w: number,
  h: number,
): Promise<void> => invoke("set_window_bounds", { x, y, w, h });

/** Subscribe to backend session-state snapshots. Returns an unlisten fn. */
export const onSessionsUpdate = (
  cb: (sessions: SessionView[]) => void,
): Promise<UnlistenFn> =>
  listen<SessionView[]>("sessions:update", (e) => cb(e.payload));

/** Flash signal: "done" (completed) or "waiting" (needs you). */
export const onFlash = (
  cb: (kind: "done" | "waiting") => void,
): Promise<UnlistenFn> =>
  listen<string>("session:flash", (e) => cb(e.payload === "waiting" ? "waiting" : "done"));
