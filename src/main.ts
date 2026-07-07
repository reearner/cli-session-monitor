import "./styles.css";
import type { SessionView, SessionKey, Config } from "./types";
import { keyId } from "./types";
import {
  getSnapshot,
  getConfig,
  setConfig,
  onSessionsUpdate,
  onFlash,
  saveWindowPos,
  saveWindowSize,
  setResizable,
  setWindowBounds,
  localHost,
  openSession,
  sessionWindowTitles,
  dismissSession,
  activeWindowCards,
  setSessionName,
  setSessionCmd,
} from "./ipc";
import { createCard } from "./session-card";
import { formatDuration } from "./timer";
import { renderSettings } from "./settings";
import { checkForUpdate } from "./updater";
import { t, setLang } from "./i18n";
import { getCurrentWindow, currentMonitor } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { LogicalSize, PhysicalPosition } from "@tauri-apps/api/dpi";

const sessionsEl = document.getElementById("sessions") as HTMLElement;
const emptyEl = document.getElementById("empty") as HTMLElement;
const settingsEl = document.getElementById("settings") as HTMLElement;
const btnSettings = document.getElementById("btn-settings") as HTMLButtonElement;
const titleEl = document.querySelector(".title") as HTMLElement;
const pillEl = document.getElementById("pill") as HTMLButtonElement;
const pillDot = pillEl.querySelector(".pill-dot") as HTMLElement;
const pillText = document.getElementById("pill-text") as HTMLElement;
const pcRun = document.getElementById("pc-run") as HTMLElement;
const pcWait = document.getElementById("pc-wait") as HTMLElement;
const pcDone = document.getElementById("pc-done") as HTMLElement;

// The full-panel size. Mutable: loaded from config on startup and updated when
// the user resizes the panel (the window is `resizable`). The ball/bar stay fixed.
let FULL = new LogicalSize(360, 640);
const BALL = new LogicalSize(60, 60);
const MIN_PANEL_W = 300;
const MIN_PANEL_H = 240;
let resizeTimer: number | undefined;

// Make it feel like a native widget, not a browser tab: no context menu, no
// reload/print/find/zoom shortcuts, no ctrl-wheel zoom.
document.addEventListener("contextmenu", (e) => e.preventDefault());
document.addEventListener("keydown", (e) => {
  const k = e.key.toLowerCase();
  if (
    k === "f5" ||
    (e.ctrlKey && ["r", "p", "f", "g", "+", "-", "=", "0"].includes(k))
  ) {
    e.preventDefault();
  }
});
document.addEventListener("wheel", (e) => { if (e.ctrlKey) e.preventDefault(); }, {
  passive: false,
});

let sessions: SessionView[] = [];
// User-assigned card names, keyed by session id (persisted in config). Applied
// as the card headline so several sessions are easy to tell apart.
let names: Record<string, string> = {};
// User-edited resume commands, keyed by session id (persisted). Overrides the
// auto-generated command so flags like --yolo survive the copy button.
let cmds: Record<string, string> = {};
let myHost = ""; // local hostname; local cards are clickable -> jump to editor
let lite = false; // persistent preference (lightweight mode)
let expanded = false; // runtime: in lite mode, temporarily expanded
// "Acknowledged": once the user opens the full panel and collapses back, stop
// the continuous waiting pulse (they've seen it). A new completion/waiting
// flash clears it so the orb alerts again.
let ack = false;

const collapsed = () => lite && !expanded;

function setAck(v: boolean): void {
  ack = v;
  document.body.classList.toggle("ack", ack);
}

async function resize(): Promise<void> {
  try {
    const win = getCurrentWindow();
    const mon = await currentMonitor();
    const sf = mon ? mon.scaleFactor : 1;
    if (collapsed()) {
      if (preExpandPos) {
        // Returning from the expanded panel: restore to the pre-expand position
        // in one atomic op (size + position) so it doesn't grow/jump.
        const bw = Math.round(BALL.width * sf);
        const bh = Math.round(BALL.height * sf);
        const p = preExpandPos;
        preExpandPos = null;
        await programmaticMove(() => setWindowBounds(p.x, p.y, bw, bh));
      } else {
        // Otherwise just shrink to the ball and KEEP the current position — the
        // position is owned by the startup restore / dock logic. (Setting it here
        // would race with and clobber the restore, sending it to the default
        // top-left corner — the "always docks left on restart" bug.)
        await programmaticMove(() => win.setSize(BALL));
      }
      return;
    }
    // Expand to the full panel, clamped on-screen, in a SINGLE atomic op so it
    // never grows at the old spot and then jumps — that two-step is what
    // flickered when expanding from a bottom/right edge. Clamping anchors the
    // growth direction (bottom ball expands upward, right one leftward).
    const fw = Math.round(FULL.width * sf);
    const fh = Math.round(FULL.height * sf);
    // Prefer the remembered home so re-expanding (incl. toggling lightweight off)
    // returns the panel where it was, not where the ball had docked. Fall back to
    // the current position the first time, before any home is known.
    const src = panelPos ?? (await win.outerPosition());
    let nx = src.x, ny = src.y;
    if (mon) {
      const mx = mon.position.x, my = mon.position.y;
      const mw = mon.size.width, mh = mon.size.height;
      nx = clamp(src.x, mx, mx + mw - fw);
      ny = clamp(src.y, my, my + mh - fh);
    }
    await programmaticMove(() => setWindowBounds(nx, ny, fw, fh));
    panelPos = new PhysicalPosition(nx, ny);
  } catch {
    /* not in tauri / no permission */
  }
}

// ---- Edge docking: an idle ball slides to the nearest screen edge as a thin
// bar; hovering it pops it back to a ball. ----
const STRIP_THICK = 22; // logical px — wide enough to show the count digits
const STRIP_LEN = 56;
const BALL_PX = 60;
const IDLE_MS = 1000;

let docked = false;
let dockEdge: "left" | "right" | "top" | "bottom" = "right";
let idleTimer: number | undefined;
let snapTimer: number | undefined;
// While we move/resize the window ourselves, ignore the resulting onMoved events
// (otherwise the snap-to-edge would fight our own dock/undock repositioning).
let suppressSnap = false;
// Last time a resize was observed. Resizing the panel from the top/left edge also
// moves its top-left (firing onMoved); we must NOT treat that as a drag-to-edge,
// or the snap would fight the user's resize and jitter — especially on big panels.
let lastResizeAt = 0;
// The ball's position right before expanding, so collapsing returns it there
// instead of leaving it at the expanded panel's top-left (which then re-docks
// from the wrong spot — the "jumps to the middle then the top" bug).
let preExpandPos: PhysicalPosition | null = null;
// The full panel's "home" position, so toggling lightweight off (or re-expanding)
// restores the panel where it was — not at the edge the ball had docked to.
// Updated whenever the full panel settles (expand / user drag-snap).
let panelPos: PhysicalPosition | null = null;
// Pending "collapse because the window lost focus". Deferred so a focus loss
// caused by DRAGGING the panel (which fires onMoved right after) can cancel it —
// app-region drags are handled by the OS and don't fire pointerdown, so we can't
// detect the grab directly.
let blurCollapseTimer: number | undefined;

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

async function programmaticMove(fn: () => Promise<void>): Promise<void> {
  suppressSnap = true;
  try {
    await fn();
  } finally {
    // A bit longer than the move/resize debounce so the OS echo events from our
    // own SetWindowPos (which can lag on a large window) are still suppressed and
    // not mistaken for a user drag — the source of the resize "jitter".
    window.setTimeout(() => {
      suppressSnap = false;
    }, 450);
  }
}

function setEdgeClass(edge: "left" | "right" | "top" | "bottom" | null): void {
  document.body.classList.remove("edge-left", "edge-right", "edge-top", "edge-bottom");
  if (edge) document.body.classList.add("edge-" + edge);
}

async function dockToEdge(anchor?: PhysicalPosition): Promise<void> {
  if (!collapsed() || docked) return;
  const win = getCurrentWindow();
  try {
    const mon = await currentMonitor();
    if (!mon) return;
    const sf = mon.scaleFactor;
    // When given an anchor (e.g. the pre-expand ball position) compute from it +
    // ball size, so collapsing docks in ONE step without ever painting the ball
    // at the expanded panel's top-left (which, for a bottom panel, is up high —
    // the "ball flies to the top then comes back" flicker).
    let px: number, py: number, sw: number, sh: number;
    if (anchor) {
      const ball = Math.round(BALL_PX * sf);
      px = anchor.x; py = anchor.y; sw = ball; sh = ball;
    } else {
      const [pos, size] = await Promise.all([win.outerPosition(), win.outerSize()]);
      px = pos.x; py = pos.y; sw = size.width; sh = size.height;
    }
    const mx = mon.position.x, my = mon.position.y;
    const mw = mon.size.width, mh = mon.size.height;
    const cx = px + sw / 2;
    const cy = py + sh / 2;
    const dl = cx - mx, dr = mx + mw - cx, dt = cy - my, db = my + mh - cy;
    const m = Math.min(dl, dr, dt, db);
    dockEdge = m === dl ? "left" : m === dr ? "right" : m === dt ? "top" : "bottom";

    const thick = Math.round(STRIP_THICK * sf);
    const len = Math.round(STRIP_LEN * sf);
    let w: number, h: number, nx: number, ny: number;
    if (dockEdge === "left" || dockEdge === "right") {
      w = thick; h = len;
      nx = dockEdge === "left" ? mx : mx + mw - thick;
      ny = Math.round(clamp(cy - len / 2, my, my + mh - len));
    } else {
      w = len; h = thick;
      ny = dockEdge === "top" ? my : my + mh - thick;
      nx = Math.round(clamp(cx - len / 2, mx, mx + mw - len));
    }
    docked = true;
    document.body.classList.add("docked");
    setEdgeClass(dockEdge);
    // Fade the orb out while the window shrinks to the strip: otherwise the round
    // ball's previous frame is shown clipped into the thin window for a frame
    // (a "half ball"). Fade it back in as the bar once the window IS the strip —
    // the 0.12s opacity transition (CSS) hides the one empty-window repaint frame
    // that a hard display toggle exposed (the "blink on drop").
    pillEl.classList.add("fading");
    await programmaticMove(() => setWindowBounds(nx, ny, w, h));
    pillEl.classList.remove("fading");
    void saveWindowPos(nx, ny); // persist so it reappears here next launch
  } catch (e) {
    console.error("dock failed", e);
  }
}

async function undock(): Promise<void> {
  if (!docked) return;
  docked = false;
  document.body.classList.remove("docked");
  setEdgeClass(null);
  const win = getCurrentWindow();
  try {
    const mon = await currentMonitor();
    const sf = mon ? mon.scaleFactor : 1;
    const ball = Math.round(BALL_PX * sf);
    const pos = await win.outerPosition();
    let nx = pos.x, ny = pos.y;
    if (mon) {
      const mx = mon.position.x, my = mon.position.y;
      const mw = mon.size.width, mh = mon.size.height;
      if (dockEdge === "left") nx = mx;
      else if (dockEdge === "right") nx = mx + mw - ball;
      else if (dockEdge === "top") ny = my;
      else ny = my + mh - ball;
    }
    await programmaticMove(() => setWindowBounds(nx, ny, ball, ball));
  } catch (e) {
    console.error("undock failed", e);
  }
}

function armIdle(): void {
  if (idleTimer) window.clearTimeout(idleTimer);
  if (collapsed() && !docked) {
    idleTimer = window.setTimeout(() => void dockToEdge(), IDLE_MS);
  }
}

// Edge-snap for the full panel: dropped near a screen edge -> align flush to it
// (keeps full size, unlike the ball which collapses to a thin bar).
const PANEL_SNAP_PX = 28; // logical px
async function snapPanelToEdge(): Promise<void> {
  if (collapsed()) return;
  // The user just dragged the full panel, so the old pre-expand ball position is
  // stale — forget it, so collapsing docks the bar near the panel's NEW spot
  // (computed from the panel's current position) instead of jumping back.
  preExpandPos = null;
  const win = getCurrentWindow();
  try {
    const [pos, size, mon] = await Promise.all([
      win.outerPosition(),
      win.outerSize(),
      currentMonitor(),
    ]);
    if (!mon) return;
    const sf = mon.scaleFactor;
    const thr = Math.round(PANEL_SNAP_PX * sf);
    const mx = mon.position.x, my = mon.position.y;
    const mw = mon.size.width, mh = mon.size.height;
    const w = size.width, h = size.height;
    let nx = pos.x, ny = pos.y;
    if (Math.abs(pos.x - mx) <= thr) nx = mx;
    else if (Math.abs(pos.x + w - (mx + mw)) <= thr) nx = mx + mw - w;
    if (Math.abs(pos.y - my) <= thr) ny = my;
    else if (Math.abs(pos.y + h - (my + mh)) <= thr) ny = my + mh - h;
    nx = clamp(nx, mx, mx + mw - w);
    ny = clamp(ny, my, my + mh - h);
    if (nx !== pos.x || ny !== pos.y) {
      await programmaticMove(() => setWindowBounds(nx, ny, w, h));
    }
    // The full panel just settled here — remember it as the new home.
    panelPos = new PhysicalPosition(nx, ny);
  } catch {
    /* ignore */
  }
}

// Tray "重置悬浮球位置": pull the widget back to a guaranteed-visible spot
// (right edge, vertically centered for the ball; centered for the full panel).
async function resetPosition(): Promise<void> {
  settingsEl.hidden = true;
  expanded = false;
  if (docked) {
    docked = false;
    document.body.classList.remove("docked");
    setEdgeClass(null);
  }
  preExpandPos = null;
  try {
    const mon = await currentMonitor();
    if (!mon) {
      await resize();
      return;
    }
    const sf = mon.scaleFactor;
    if (collapsed()) {
      document.body.classList.add("ball");
      const ball = Math.round(BALL_PX * sf);
      const x = mon.position.x + mon.size.width - ball;
      const y = mon.position.y + Math.round((mon.size.height - ball) / 2);
      await programmaticMove(() => setWindowBounds(x, y, ball, ball));
      armIdle();
    } else {
      const fw = Math.round(FULL.width * sf);
      const fh = Math.round(FULL.height * sf);
      const x = mon.position.x + Math.round((mon.size.width - fw) / 2);
      const y = mon.position.y + Math.round((mon.size.height - fh) / 2);
      await programmaticMove(() => setWindowBounds(x, y, fw, fh));
      panelPos = new PhysicalPosition(x, y); // reset the home too
    }
  } catch {
    /* ignore */
  }
}

function counts() {
  let running = 0,
    waiting = 0,
    done = 0,
    idle = 0;
  for (const s of sessions) {
    if (s.status === "running") running++;
    else if (s.status === "waiting") waiting++;
    else if (s.status === "done") done++;
    else if (s.status === "idle") idle++;
  }
  return { running, waiting, done, idle };
}

function updatePill(): void {
  const { running, waiting, done, idle } = counts();
  const active = running + waiting + done;
  // Ball: single number = active sessions.
  pillText.textContent = active ? String(active) : "";
  // Docked bar — color-matched to the cards so the strip can't be misread:
  // ▶running (blue), !waiting (amber = needs your input), ✓done (green = replied).
  // Idle is omitted from the thin bar (shown only in the full panel).
  pcRun.textContent = running ? `▶${running}` : "";
  pcWait.textContent = waiting ? `!${waiting}` : "";
  pcDone.textContent = done ? `✓${done}` : "";
  void idle;
  // Color the orb / bar by the most-urgent state: needs-you (amber) > running
  // (blue) > replied (green) > idle — same priority as the card ordering.
  const cls = waiting ? "s-wait" : running ? "s-run" : done ? "s-done" : "s-idle";
  pillEl.className = "pill " + cls;
  pillDot.className = "pill-dot " + cls;
}

// The session that most recently fired a flash (completion / waiting). Its card
// pulses so you can spot which one it was when you open the panel. Cleared when
// you click that card or after a short while.
let alertKey: string | null = null;
let alertTimer: number | undefined;
function setAlert(key: SessionKey): void {
  alertKey = keyId(key);
  if (alertTimer) window.clearTimeout(alertTimer);
  alertTimer = window.setTimeout(() => {
    alertTimer = undefined;
    alertKey = null;
    render();
  }, 12000);
  render();
}

// Inline-edit the card headline. Enter/blur commits, Escape cancels. Blank name
// clears the custom name (reverts to the project folder name). Persisted by
// session id so it sticks across restarts and --resume.
function startRename(v: SessionView, card: HTMLElement): void {
  const nameEl = card.querySelector<HTMLElement>(".project");
  if (!nameEl) return;
  const id = v.key.session_id;
  const input = document.createElement("input");
  input.className = "rename-input";
  input.value = names[id] ?? "";
  input.placeholder = nameEl.textContent ?? "";
  input.spellcheck = false;
  input.maxLength = 40;
  // Don't let clicks/keys inside the input reach the card (jump / space-scroll).
  const swallow = (e: Event) => e.stopPropagation();
  input.addEventListener("click", swallow);
  input.addEventListener("mousedown", swallow);
  input.addEventListener("keydown", swallow);
  nameEl.replaceWith(input);
  input.focus();
  input.select();
  editing = true; // pause re-renders so a snapshot push can't wipe this input

  let done = false;
  const commit = (save: boolean) => {
    if (done) return;
    done = true;
    editing = false;
    if (save) {
      const val = input.value.trim();
      if (val) names[id] = val;
      else delete names[id];
      void setSessionName(id, val).catch(() => {});
    }
    render();
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      commit(true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      commit(false);
    }
  });
  input.addEventListener("blur", () => commit(true));
}

// A cheap signature of everything the card list actually paints. The backend
// pushes a snapshot whenever session STATE changes — but that includes changes to
// fields the card doesn't show (e.g. last_event_at ticks on every Claude tool
// call). Rebuilding the whole list for an identical-looking snapshot flashed the
// (transparent) window. We render only when this signature changes.
function sessionsSig(list: SessionView[]): string {
  return list
    .map(
      (v) =>
        `${keyId(v.key)}|${v.status}|${v.run_started_at ?? ""}|${v.last_duration_ms ?? ""}|${
          v.timing_reliable ? 1 : 0
        }|${v.cwd}|${names[v.key.session_id] ?? ""}`,
    )
    .join(";");
}
let lastRenderSig = "";
// True while an inline editor (rename / resume-command) is open. A full re-render
// tears down all cards (replaceChildren), destroying the open <input> mid-typing
// — the "screen flickers and my edit is interrupted" bug. Skip renders while
// editing; the commit re-renders once with the final state.
let editing = false;

// The auto-generated resume command for a card. Every card — live or
// discovered-from-disk — is keyed by a real session id, so it resumes that exact
// session. A remembered custom command (cmds[id]) overrides this. The `--last`
// fallback only ever fires for a hypothetical id-less placeholder.
function defaultResumeCmd(v: SessionView): string {
  const id = v.key.session_id;
  const isReal = !id.startsWith("disc:");
  if (isReal) return v.source === "codex" ? `codex resume ${id}` : `claude --resume ${id}`;
  return v.source === "codex" ? "codex resume --last" : "claude --continue";
}

// Right-click the ⧉ button to edit the command this card remembers. Prefilled
// with the current effective command; Enter saves (persisted by session id),
// Escape cancels, blank reverts to the auto default.
function startCmdEdit(v: SessionView, card: HTMLElement): void {
  const id = v.key.session_id;
  const def = defaultResumeCmd(v);
  const input = document.createElement("input");
  input.className = "cmd-input";
  input.value = cmds[id] ?? def;
  input.placeholder = def;
  input.spellcheck = false;
  const swallow = (e: Event) => e.stopPropagation();
  input.addEventListener("click", swallow);
  input.addEventListener("mousedown", swallow);
  card.insertBefore(input, card.querySelector(".winline"));
  input.focus();
  input.select();
  editing = true; // pause re-renders so a snapshot push can't wipe this input

  let done = false;
  const commit = (save: boolean) => {
    if (done) return;
    done = true;
    editing = false;
    if (save) {
      const val = input.value.trim();
      const keep = val && val !== def ? val : "";
      if (keep) cmds[id] = keep;
      else delete cmds[id];
      void setSessionCmd(id, keep).catch(() => {});
    }
    input.remove();
    render(); // reflect final state (and any snapshot missed while editing)
  };
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      commit(true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      commit(false);
    }
  });
  input.addEventListener("blur", () => commit(true));
}

function render(): void {
  if (editing) return; // don't tear down an open inline editor mid-typing
  // full card list
  sessionsEl.replaceChildren();
  for (const v of sessions) {
    const card = createCard(v, myHost, names[v.key.session_id] ?? "");
    if (alertKey && keyId(v.key) === alertKey) card.classList.add("alerted");
    // Rename: replace the headline with an inline input; commit persists the name
    // by session id (survives restarts / --resume) and re-renders.
    card.querySelector<HTMLButtonElement>(".card-rename")?.addEventListener("click", (e) => {
      e.stopPropagation();
      startRename(v, card);
    });
    // Close button: hide this card (reappears on fresh activity). Stop the click
    // from bubbling to the card's jump-to-editor handler.
    card.querySelector<HTMLButtonElement>(".card-close")?.addEventListener(
      "click",
      (e) => {
        e.stopPropagation();
        void dismissSession(v.key);
      },
    );
    // Copy the resume command. Left-click copies the effective command (a
    // remembered custom one if set, else the auto default); right-click edits and
    // remembers it — so flags like --yolo the default drops are preserved.
    const resumeBtn = card.querySelector<HTMLButtonElement>(".card-resume");
    if (resumeBtn) {
      if (cmds[v.key.session_id]) resumeBtn.classList.add("has-cmd");
      resumeBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        const cmd = cmds[v.key.session_id] ?? defaultResumeCmd(v);
        const btn = e.currentTarget as HTMLButtonElement;
        void navigator.clipboard
          .writeText(cmd)
          .then(() => {
            const orig = btn.textContent;
            btn.textContent = "✓";
            window.setTimeout(() => {
              btn.textContent = orig;
            }, 1000);
          })
          .catch(() => {});
      });
      resumeBtn.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        e.stopPropagation();
        startCmdEdit(v, card);
      });
    }
    // Click to jump to the session's editor window. Works for local sessions and
    // for remote ones opened via VS Code/Cursor Remote-SSH (window runs locally).
    if (v.cwd) {
      const remote = !!myHost && v.host !== myHost;
      // Click: focus the matching window if open, else open the SPECIFIC editor
      // remembered for this dir (one editor, not a try-everything chain). Remote
      // sessions only focus (can't open a remote folder locally).
      const create = !remote;
      card.classList.add("openable");
      card.tabIndex = 0; // keyboard-focusable
      card.setAttribute("role", "button");
      card.title = remote
        ? t("card.clickRemote")
        : t("card.clickLocal", { dir: v.cwd });
      const activate = () => {
        if (alertKey === keyId(v.key)) {
          alertKey = null;
          if (alertTimer) {
            window.clearTimeout(alertTimer);
            alertTimer = undefined;
          }
          card.classList.remove("alerted");
        }
        openSession(v.cwd, v.host, create).catch(() => {
          // No focusable window found — give a brief visual nudge instead of
          // failing silently.
          card.classList.add("jump-failed");
          window.setTimeout(() => card.classList.remove("jump-failed"), 1200);
        });
      };
      card.addEventListener("click", activate);
      card.addEventListener("keydown", (e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          activate();
        }
      });
    }
    sessionsEl.append(card);
  }
  emptyEl.textContent = t("app.empty");
  emptyEl.hidden = sessions.length > 0;
  updatePill();
  void refreshWindowLines();
}

// Static texts that live in index.html (not re-rendered each tick) — set from the
// active language at startup and whenever the language changes.
function applyStaticTexts(): void {
  btnSettings.title = t("app.settings");
}

// Fill each card's window line with the editor window it maps to (one batched
// backend call that enumerates windows once). Shows the launch dir + the window.
async function refreshWindowLines(): Promise<void> {
  const snap = sessions;
  if (snap.length === 0) return;
  let titles: (string | null)[] = [];
  try {
    titles = await sessionWindowTitles(snap.map((s) => ({ cwd: s.cwd, host: s.host })));
  } catch {
    return;
  }
  snap.forEach((v, i) => {
    const card = sessionsEl.querySelector<HTMLElement>(
      `.card[data-key="${CSS.escape(keyId(v.key))}"]`,
    );
    const line = card?.querySelector<HTMLElement>(".winline");
    if (!card || !line) return;
    const titleStr = titles[i];
    if (titleStr) {
      // strip the trailing " - Visual Studio Code" / " — Cursor" app suffix
      const short = titleStr.replace(/\s*[-—]\s*(Visual Studio Code|Cursor)\s*$/i, "");
      line.textContent = "🪟 " + short;
      line.title = t("card.window", { title: titleStr });
      line.hidden = false;
    } else {
      line.textContent = t("card.noWindow");
      line.title = t("card.noWindowTitle");
      line.hidden = false;
    }
  });
  applyActiveHighlight();
  lastRenderSig = sessionsSig(sessions);
}

// Highlight the card(s) whose editor window is the current foreground window.
// The backend resolves it to the single most-specific group (so a parent window
// lights up only the card opened there, not every child session under it).
let activeKeys = new Set<string>();
function applyActiveHighlight(): void {
  sessionsEl.querySelectorAll<HTMLElement>(".card").forEach((card) => {
    card.classList.toggle("active-window", activeKeys.has(card.dataset.key ?? ""));
  });
}
async function pollForeground(): Promise<void> {
  try {
    const r = await activeWindowCards();
    if (r.own) return; // our own widget is focused — keep the previous highlight
    activeKeys = new Set(r.keys.map(keyId));
  } catch {
    return;
  }
  applyActiveHighlight();
}

// Collapse the expanded panel straight back to the docked bar in one step,
// anchored at the pre-expand position — avoids any intermediate frame at the
// panel's top-left (the top-flicker).
function collapseNow(): void {
  if (idleTimer) window.clearTimeout(idleTimer);
  expanded = false;
  settingsEl.hidden = true;
  setAck(true);
  void setResizable(false); // ball/bar are fixed-size
  // Switch to ball mode (transparent bg, panel content hidden) but keep the pill
  // HIDDEN until the window is resized to the bar — otherwise the pill paints
  // centered in the still-panel-sized window (flashing at the panel's corner).
  document.body.classList.add("ball");
  pillEl.hidden = true;
  const anchor = preExpandPos ?? undefined;
  preExpandPos = null;
  void (async () => {
    await dockToEdge(anchor);
    pillEl.hidden = false;
  })();
}

function applyMode(): void {
  if (idleTimer) window.clearTimeout(idleTimer);
  if (!collapsed() && docked) {
    docked = false;
    document.body.classList.remove("docked");
    setEdgeClass(null);
  }
  document.body.classList.toggle("ball", collapsed());
  pillEl.hidden = !collapsed();
  // Only the full panel is user-resizable; the ball / docked bar are fixed-size.
  void setResizable(!collapsed());
  void resize();
  armIdle();
}

// The default collapsed state is the docked BAR; clicking it expands straight to
// the panel — no intermediate "hover turns it into a ball" step. The round ball
// only pops out on a notification flash (see onFlash), then re-docks. Hovering
// just holds off the idle re-dock so the bar/orb stays put while you aim at it.
document.documentElement.addEventListener("mouseenter", () => {
  if (idleTimer) window.clearTimeout(idleTimer);
});
document.documentElement.addEventListener("mouseleave", () => {
  armIdle();
});
// A press = active interaction: drop the idle auto-dock so its resize can't fire
// between this press and its release and swallow the click.
document.addEventListener("pointerdown", () => {
  if (idleTimer) window.clearTimeout(idleTimer);
});
document.addEventListener("mousemove", () => {
  // Cursor is over the orb — keep it put (and clickable); don't let the idle
  // timer dock it out from under the pointer. It re-docks on mouseleave.
  if (collapsed() && !docked && idleTimer) {
    window.clearTimeout(idleTimer);
    idleTimer = undefined;
  }
});

function tickTimers(): void {
  const now = Date.now();
  sessionsEl.querySelectorAll<HTMLElement>(".timer[data-start]").forEach((t) => {
    const start = Number(t.dataset.start);
    if (Number.isFinite(start)) t.textContent = formatDuration(now - start);
  });
}

// Flash window per kind. "waiting" (the CLI is asking you something) lingers
// noticeably longer than a plain "replied" so a question is harder to miss.
const FLASH_MS = { done: 2400, waiting: 6000 } as const;
let flashTimer: number | undefined;
function flash(kind: "done" | "waiting"): void {
  const b = document.body;
  b.classList.remove("flash-done", "flash-waiting");
  // force reflow so the animation restarts even on rapid repeats
  void b.offsetWidth;
  b.classList.add(kind === "waiting" ? "flash-waiting" : "flash-done");
  if (flashTimer) window.clearTimeout(flashTimer);
  flashTimer = window.setTimeout(
    () => b.classList.remove("flash-done", "flash-waiting"),
    FLASH_MS[kind],
  );
}

// Manual drag for the ball/bar. The .pill is deliberately NOT an OS drag region
// (those swallow the click on any micro-movement), so a press that stays put is a
// reliable click, and a press that moves past a small threshold starts an OS
// window drag instead. This is what makes clicking the docked bar dependable.
//
// Uses mouse events with WINDOW-level listeners attached on mousedown: while a
// button is held, the browser keeps delivering mousemove to the window even when
// the cursor leaves the (very thin) docked-bar window — so the move threshold is
// reached and the drag starts. (Element-level pointermove / pointer capture is
// unreliable here: moving off the few-pixel transparent strip loses the events.)
let pillDragged = false;
pillEl.addEventListener("mousedown", (e) => {
  // When docked, the thin bar uses an OS drag region (CSS) instead — manual
  // move-threshold detection can't work on a few-pixel strip.
  if (e.button !== 0 || docked) return;
  const sx = e.screenX;
  const sy = e.screenY;
  pillDragged = false;
  const onMove = (ev: MouseEvent) => {
    if (Math.abs(ev.screenX - sx) > 4 || Math.abs(ev.screenY - sy) > 4) {
      pillDragged = true;
      cleanup();
      void getCurrentWindow().startDragging();
    }
  };
  const cleanup = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", cleanup);
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", cleanup);
});

// pill click -> expand (remember where the ball was so we can return it there).
// Skip the click that ends a drag.
pillEl.addEventListener("click", async () => {
  if (pillDragged) {
    pillDragged = false;
    return;
  }
  try {
    preExpandPos = await getCurrentWindow().outerPosition();
  } catch {
    preExpandPos = null;
  }
  expanded = true;
  applyMode();
});
// in lite mode, clicking the title collapses back to the docked bar
titleEl.addEventListener("click", () => {
  if (lite && expanded) collapseNow();
});

btnSettings.addEventListener("click", () => {
  if (!settingsEl.hidden) {
    settingsEl.hidden = true;
  } else {
    settingsEl.hidden = false;
    void renderSettings(settingsEl);
  }
});

// First-run onboarding: the empty-state hint is clickable and opens Settings, so
// new users can enable the Claude Code integration without hunting for the gear.
emptyEl.style.cursor = "pointer";
emptyEl.addEventListener("click", () => {
  if (settingsEl.hidden) {
    settingsEl.hidden = false;
    void renderSettings(settingsEl);
  }
});

// settings.ts dispatches this when the lightweight toggle changes.
// Don't collapse on the spot (the user is still in settings) — stay expanded;
// it'll collapse to the pill once the window loses focus.
window.addEventListener("csm:lightweight", (e) => {
  lite = (e as CustomEvent<boolean>).detail;
  expanded = true;
  applyMode();
});

// settings.ts dispatches this when the language changes — re-resolve and re-render.
window.addEventListener("csm:lang", (e) => {
  setLang((e as CustomEvent<string>).detail);
  applyStaticTexts();
  render();
});

async function init(): Promise<void> {
  let savedX: number | null = null;
  let savedY: number | null = null;
  let onboardCfg: Config | null = null;
  try {
    const cfg = await getConfig();
    lite = cfg.lightweight;
    savedX = cfg.win_x;
    savedY = cfg.win_y;
    names = cfg.session_names ?? {};
    cmds = cfg.session_cmds ?? {};
    setLang(cfg.language);
    FULL = new LogicalSize(
      Math.max(MIN_PANEL_W, cfg.panel_w),
      Math.max(MIN_PANEL_H, cfg.panel_h),
    );
    if (!cfg.onboarded) onboardCfg = cfg;
  } catch {
    lite = false;
  }
  applyStaticTexts();
  applyMode();

  // First-ever launch: open Settings once so new users can enable the Claude Code
  // integration instead of staring at an empty panel; remember we've done it.
  if (onboardCfg) {
    settingsEl.hidden = false;
    void renderSettings(settingsEl);
    onboardCfg.onboarded = true;
    void setConfig(onboardCfg);
  }

  try {
    myHost = await localHost();
  } catch {
    myHost = "";
  }

  // Restore last position so the widget reappears where it was last docked —
  // but clamp into the current monitor so a stale off-screen position (monitor
  // changed / resolution differs) can't leave it invisible.
  if (lite && savedX != null && savedY != null) {
    try {
      const mon = await currentMonitor();
      let x = savedX, y = savedY;
      if (mon) {
        const ball = Math.round(BALL_PX * mon.scaleFactor);
        x = clamp(savedX, mon.position.x, mon.position.x + mon.size.width - ball);
        y = clamp(savedY, mon.position.y, mon.position.y + mon.size.height - ball);
      }
      await programmaticMove(() =>
        getCurrentWindow().setPosition(new PhysicalPosition(x, y)),
      );
    } catch {
      /* ignore */
    }
  }

  try {
    sessions = await getSnapshot();
  } catch {
    sessions = [];
  }
  render();

  try {
    await onSessionsUpdate((s) => {
      sessions = s;
      // Skip the full rebuild when nothing the card paints actually changed —
      // avoids the transparent-window flash on every backend push (e.g. a running
      // session's frequent tool-call events). The live timer keeps ticking via
      // tickTimers, which reads run_started_at without a re-render.
      if (sessionsSig(s) === lastRenderSig) return;
      render();
    });
    await onFlash(async (kind, key) => {
      setAck(false); // a fresh completion/waiting -> alert again
      // Mark which card it was so it pulses (green=replied, amber=needs you) —
      // so on expanding the panel you can tell WHICH session just changed.
      setAlert(key);
      // Keep the orb steady and clickable for the WHOLE flash. A docked bar pops
      // back to the round ball first (its glow would otherwise be a clipped
      // square). Either way, hold off the idle re-dock until the flash is well
      // over — otherwise the ~1s idle timer can dock the ball out from under a
      // click drawn in by the flash ("clicking the ball does nothing").
      if (collapsed()) {
        if (docked) await undock();
        if (idleTimer) window.clearTimeout(idleTimer);
        // Keep the orb out for the whole flash (waiting lingers longer) plus a
        // small tail, so it never re-docks mid-flash.
        idleTimer = window.setTimeout(() => void dockToEdge(), FLASH_MS[kind] + 200);
      }
      flash(kind);
    });

    // Tray "重置悬浮球位置" -> recover a lost widget.
    await listen("csm:reset-pos", () => void resetPosition());

    // Drag-to-edge: after the user drops the ball/bar anywhere, snap it to the
    // nearest screen edge and re-orient (vertical on left/right, horizontal on
    // top/bottom) — no need to first morph it back into a ball.
    await getCurrentWindow().onMoved(() => {
      if (suppressSnap) return;
      // A user move => this is a drag, not a click-away: cancel any pending
      // collapse-on-blur so dragging the panel doesn't shrink it to a ball.
      if (blurCollapseTimer) {
        window.clearTimeout(blurCollapseTimer);
        blurCollapseTimer = undefined;
      }
      if (snapTimer) window.clearTimeout(snapTimer);
      snapTimer = window.setTimeout(() => {
        if (collapsed()) {
          // ball/bar: dock to the nearest edge (re-orienting)
          if (docked) {
            docked = false;
            document.body.classList.remove("docked");
            setEdgeClass(null);
          }
          // This fires only for a USER drag (our own moves set suppressSnap), so
          // bind the panel's home to where the widget was dragged — expanding then
          // opens where it now is, not at its old spot.
          void dockToEdge().then(async () => {
            try {
              panelPos = await getCurrentWindow().outerPosition();
            } catch {
              /* ignore */
            }
          });
        } else if (Date.now() - lastResizeAt >= 600) {
          // full panel: snap flush to a nearby edge (keeps full size). Skip when a
          // resize just happened — resizing from the top/left edge moves the
          // top-left too, and snapping then would fight the user's drag (jitter).
          void snapPanelToEdge();
        }
      }, 300);
    });

    // Remember the user's manual resize of the full panel. Our own resizes set
    // suppressSnap (and the ball/bar are collapsed), so those are ignored.
    await getCurrentWindow().onResized(() => {
      lastResizeAt = Date.now();
      if (suppressSnap || collapsed()) return;
      if (resizeTimer) window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(async () => {
        try {
          const [size, mon] = await Promise.all([
            getCurrentWindow().outerSize(),
            currentMonitor(),
          ]);
          const sf = mon ? mon.scaleFactor : 1;
          const w = Math.max(MIN_PANEL_W, Math.round(size.width / sf));
          const h = Math.max(MIN_PANEL_H, Math.round(size.height / sf));
          FULL = new LogicalSize(w, h);
          void saveWindowSize(w, h);
        } catch {
          /* ignore */
        }
      }, 300);
    });

    // In lite mode, leaving the expanded view (window loses focus) re-collapses
    // it back to the pill.
    await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      // Focus came back (e.g. drag finished) — abandon any pending collapse.
      if (focused) {
        if (blurCollapseTimer) {
          window.clearTimeout(blurCollapseTimer);
          blurCollapseTimer = undefined;
        }
        return;
      }
      // Lost focus: usually the user clicked away -> collapse to the ball. But
      // DRAGGING the panel also blurs it (then fires onMoved, which cancels this).
      // Defer so a drag can cancel; a real click-away has no onMoved and collapses.
      if (lite && expanded && !blurCollapseTimer) {
        blurCollapseTimer = window.setTimeout(() => {
          blurCollapseTimer = undefined;
          if (lite && expanded) collapseNow();
        }, 300);
      }
    });
  } catch (e) {
    console.error("subscribe failed", e);
  }

  window.setInterval(tickTimers, 1000);
  void pollForeground();
  window.setInterval(() => void pollForeground(), 1200);
  // Keep each card's editor-window line fresh on its own cadence (in-place text
  // update, no rebuild) — decoupled from session pushes now that we skip renders
  // that don't change the card content. Only while the panel is visible.
  window.setInterval(() => {
    if (!collapsed()) void refreshWindowLines();
  }, 3000);

  // Check for an app update in the background (shows a banner if one is found).
  void checkForUpdate();
}

void init();
