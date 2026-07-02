import type { SessionView, Status } from "./types";
import { keyId, sourceLabel } from "./types";
import { formatDuration } from "./timer";
import { t } from "./i18n";

function el(tag: string, cls?: string, text?: string): HTMLElement {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

// `done` = the CLI finished its turn (your turn to type); `waiting` = it's blocked
// needing your choice/approval before it can continue. Shown distinctly (label +
// color) so you can tell "replied" from "needs a decision" at a glance.
function statusText(s: Status): string {
  return t(
    s === "idle"
      ? "status.idle"
      : s === "running"
        ? "status.running"
        : s === "done"
          ? "status.done"
          : "status.waiting",
  );
}

/**
 * Build a session card. Running cards carry `data-start` on the `.timer` so the
 * 1s tick in main.ts can update the elapsed time without a full re-render.
 */
export function createCard(v: SessionView, localHost = "", customName = ""): HTMLElement {
  const card = el("div", `card status-${v.status} src-${v.source}`);
  card.dataset.key = keyId(v.key);

  // Real sessions carry a UUID; discovered placeholders are keyed by dir
  // ("disc:..."), which isn't a resumable session id.
  const real = !v.key.session_id.startsWith("disc:");

  // Headline = a user-assigned name if set (persisted by session id, so it sticks
  // across restarts / --resume), else the PROJECT name (the dir's last segment).
  // The parent dir (below) and the full path (hover) tell same-named projects
  // apart. The CLI kind moves to the footer.
  const project = v.cwd.split(/[\\/]/).filter(Boolean).pop() || v.cwd || "—";
  const parent = v.cwd.replace(/[\\/][^\\/]+[\\/]*$/, "");

  const head = el("div", "card-head");
  const label = customName.trim() || project;
  const name = el("span", customName.trim() ? "project named" : "project", label);
  name.title = v.cwd;
  head.append(el("span", "dot"), name);
  // Short session id — tells apart two cards that share a directory (e.g. several
  // agents in one editor window).
  if (real) {
    const sid = el("span", "sid", "#" + v.key.session_id.slice(-6));
    sid.title = v.key.session_id;
    head.append(sid);
  }
  // Rename (wired in main.ts): give this card a custom name that persists by
  // session id — so you can tell several sessions apart at a glance.
  if (real) {
    const rename = el("button", "card-rename", "✎");
    rename.title = t("card.rename");
    head.append(rename);
  }
  // Copy the resume command (wired in main.ts) so you can paste it into a terminal.
  // Real sessions copy an exact `--resume <id>`; grey/discovered cards (dir only,
  // no id) copy a "continue the most recent session in this folder" command.
  const resume = el("button", "card-resume", "⧉");
  resume.title = real ? t("card.copyResume") : t("card.copyResumeDir");
  head.append(resume);
  // Close button — hides this card (wired in main.ts). Stops propagation there
  // so it doesn't also trigger the card's jump-to-editor click.
  const close = el("button", "card-close", "×");
  close.title = t("card.close");
  head.append(close);

  card.append(head);

  // Parent directory only (so the project name isn't repeated). Hidden when there
  // is none (e.g. a bare folder name).
  if (parent && parent !== v.cwd) {
    const dir = el("div", "cwd", "▸ " + parent);
    dir.title = t("card.launchDir", { dir: v.cwd });
    card.append(dir);
  }

  // Filled asynchronously by main.ts with the matched editor window (or none).
  const winline = el("div", "winline", "");
  winline.hidden = true;
  card.append(winline);

  // Footer: CLI kind (demoted), remote host (only when remote), status, timer.
  const meta = el("div", "meta");
  meta.append(el("span", "src-tag", sourceLabel(v.source)));
  if (v.host && v.host !== localHost) {
    meta.append(el("span", "host", v.host));
  }
  meta.append(el("span", "status", statusText(v.status)));

  const timer = el("span", "timer");
  const live = v.status === "running" || v.status === "waiting";
  if (live && v.run_started_at != null) {
    timer.dataset.start = String(v.run_started_at);
    timer.textContent = formatDuration(Date.now() - v.run_started_at);
  } else if (v.last_duration_ms != null) {
    timer.textContent = formatDuration(v.last_duration_ms);
  } else {
    timer.textContent = "—";
  }
  meta.append(timer);

  // Codex (and any source without a clean start) reports unreliable timing.
  if (!v.timing_reliable && v.status !== "running") {
    meta.append(el("span", "badge", t("card.estimate")));
  }

  card.append(meta);
  return card;
}
