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
export function createCard(v: SessionView): HTMLElement {
  const card = el("div", `card status-${v.status} src-${v.source}`);
  card.dataset.key = keyId(v.key);

  // Real sessions carry a UUID; discovered placeholders are keyed by dir
  // ("disc:..."), which isn't a resumable session id.
  const real = !v.key.session_id.startsWith("disc:");

  // Lead with the PROJECT name (the dir's last segment) — that's what identifies
  // which session this is. The CLI kind (Claude/Codex) is secondary, so it's a
  // small muted tag rather than the headline.
  const project = v.cwd.split(/[\\/]/).filter(Boolean).pop() || v.cwd || "—";
  const head = el("div", "card-head");
  const name = el("span", "project", project);
  name.title = v.cwd;
  head.append(el("span", "dot"), name, el("span", "src-tag", sourceLabel(v.source)));
  if (v.host) head.append(el("span", "host", v.host));
  // Short session id — tells apart two cards that share a directory (e.g. several
  // agents in one editor window).
  if (real) {
    const sid = el("span", "sid", "#" + v.key.session_id.slice(-6));
    sid.title = v.key.session_id;
    head.append(sid);
  }
  // Copy the resume command (wired in main.ts) so you can paste it into the exact
  // terminal you want — handy when several agents share one window/dir.
  if (real) {
    const resume = el("button", "card-resume", "⧉");
    resume.title = t("card.copyResume");
    head.append(resume);
  }
  // Close button — hides this card (wired in main.ts). Stops propagation there
  // so it doesn't also trigger the card's jump-to-editor click.
  const close = el("button", "card-close", "×");
  close.title = t("card.close");
  head.append(close);

  const cwd = el("div", "cwd", "▸ " + v.cwd);
  cwd.title = t("card.launchDir", { dir: v.cwd });

  // Filled asynchronously by main.ts with the matched editor window (or none).
  const winline = el("div", "winline", "");
  winline.hidden = true;

  const meta = el("div", "meta");
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

  card.append(head, cwd, winline, meta);
  return card;
}
