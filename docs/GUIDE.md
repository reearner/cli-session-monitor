# User Guide

A step-by-step walkthrough of everything the widget can do. New here? Start with
**Quick start**, then dip into the section you need.

> 中文: [GUIDE.zh.md](GUIDE.zh.md)
>
> Language: the UI follows your system locale automatically (English / 中文). You
> can force it under **Settings → Language**.

---

## Quick start

1. **Launch** the app. It appears as a small panel (and a tray icon).
2. Open **Settings** (the ⚙ button) → **Integrations** → next to *Claude Code*
   click **Install**. That's the only setup step. (Codex needs nothing — see
   below.)
3. Use Claude Code / Codex as usual. Each session shows up as a **card** with its
   status and a live timer; you get a desktop notification when a turn finishes.

> Just want to see what it looks like first? Launch with `CSM_DEMO=1` for a
> self-contained demo (fake data, nothing read or sent).

---

## Connecting your CLIs

### Claude Code (one click, reversible)

**Settings → Integrations → Claude Code → Install.** This appends a small
reporting *hook* to `~/.claude/settings.json`. It is:

- **Append-only** — your existing settings and hooks are kept.
- **Backed up first** — a timestamped copy is written before any change.
- **Reversible** — **Uninstall** removes only the entries this app added.

The row shows **Installed ✓** when active.

### Codex (automatic — nothing to install)

Codex is watched **read-only** by tailing its session files
(`~/.codex/sessions/**/rollout-*.jsonl`). No config change, no install. The row
shows **Auto-monitored ✓**. (If an older version of this tool added a `notify`
entry, **Remove old notify** cleans it up.)

---

## Reading the cards

Each session is one card:

- **Status dot + label** — four states, color-coded:
  - 🔵 **Running** — the agent is working.
  - 🟢 **Replied** — it finished its turn; your turn to type.
  - 🟡 **Needs your input** — it's blocked waiting for your choice/approval (a
    Claude permission prompt / a Codex approval) before it can continue.
  - ⚪ **Idle** — finished a while ago (fades after the "keep finished" minutes).
  - The card that *just* changed pulses briefly so you can spot it. (Codex has no
    "needs your input" state — its log can't reliably tell an approval pause from
    a long-running command.)
- **Live timer** — counts up while running; shows the last run's duration when
  done. Codex turns carry an **`est.`** badge because their timing is approximate.
- **`▸` launch directory** and **`🪟` matched editor window** (or *No matching
  window*).
- **Highlight** — the card whose editor window is the one you're **currently
  focused on** is highlighted, so you can tell at a glance which session you're
  looking at.
- **Close (×)** — hides a card you don't care about. It comes back automatically
  if that session does something new.

---

## Jump to the editor (click a card)

**Click a card** to bring its **Cursor / VS Code** window to the front — including
windows opened over **Remote-SSH** (see below). The app remembers which editor a
folder uses and reopens that one.

### Optimize jump (only if two folders share a name)

Matching uses the **folder shown in the window title**. If you have two windows
open on folders with the **same name** (e.g. two different `app` folders), the app
can't tell them apart by title alone.

Fix it once: **Settings → Editor jump → Optimize jump.** This writes a
`window.title` setting into VS Code / Cursor so the title carries the **full
path**. It's backed up, won't override a title you already customized, and
**Revert** undoes it. After this, both jump and the active-window highlight are
exact.

> Limitation: jump focuses a **window**, not an individual terminal tab. Several
> CLIs in one editor window all map to that window.

---

## Floating ball & edge docking

Turn on **Settings → Lightweight**. Now:

- **Click the title bar** to collapse the panel into a **floating ball**.
- The ball **docks to the nearest screen edge** as a thin bar showing counts
  (**▶** running / **!** awaiting input).
- **Hover** the bar to pop it back to a ball; **click** to reopen the full panel;
  **drag** it anywhere to re-dock to the new edge.
- On a completion / waiting event it **pulses** to get your attention.
- Its position is **remembered across restarts**. Lost it? Tray → **Reset ball
  position**.

---

## Remote monitoring (sessions on another machine)

You can watch CLI sessions running on a **remote server**, with no SSH tunnel of
your own. The link is an **[ntfy](https://ntfy.sh) topic**: the desktop app
**subscribes** to one topic; a small **agent** on the remote **publishes** to that
**same** topic.

1. **Settings → Remote.** Set a **hard-to-guess topic** (or click *Export* and one
   is generated). Optionally set a self-hosted relay URL and an access token.
2. Click **Export remote script** → writes `remote-agent.sh` (to
   `~/.cli-session-monitor/`) with the topic/URL/token **baked in**.
3. Get the two small binaries onto the remote, next to `remote-agent.sh`. Easiest:
   download **`csm-remote-x86_64-linux.tar.gz`** from the
   [latest release](https://github.com/reearner/cli-session-monitor/releases/latest)
   and extract it there (static musl binaries — **no Rust needed**, runs on any
   x86_64 Linux):
   ```bash
   tar -xzf csm-remote-x86_64-linux.tar.gz   # -> ./csm-agent and ./session-reporter
   ```
   (Alternatives: set `CSM_AGENT_BIN` / `CSM_REPORTER_BIN` to existing paths, or run
   inside a checkout of this repo with Rust installed so it can `cargo build` them.)
4. On the remote: `bash remote-agent.sh`.
5. Back on the desktop (subscribed to that topic), the remote's sessions appear,
   labeled with the remote **host**. (Just set the topic? Restart the app so it
   subscribes.)

Run the **same topic** on every remote you want to see in one view — the desktop
shows the one topic it's subscribed to. The exported script differs between topics
only in the `CSM_RELAY_TOPIC=` line.

**What it monitors.** The agent covers **both** CLIs: **Codex automatically** (it
reads the rollout files), and **Claude Code** once its reporting hooks are
installed on that remote host. Install them in one step:

```bash
bash remote-agent.sh --install-claude   # install Claude hooks here, then run
```

Plain `bash remote-agent.sh` runs the agent without touching Claude (Codex only).

**Keep it running — it's a long-running relay, not a one-shot.** `csm-agent` stays
in the foreground watching for session activity and publishing events as they
happen; if it exits, the desktop stops getting updates. So leave it running, or
start it detached:

```bash
# background with nohup (logs to a file)
nohup bash remote-agent.sh > ~/csm-agent.log 2>&1 &
tail -f ~/csm-agent.log        # confirm the "publishing …" lines; Ctrl-C leaves it running

# …or in a tmux session you can reattach to
tmux new -s csm 'bash remote-agent.sh'     # detach: Ctrl-b d  ·  reattach: tmux attach -t csm
```

**One agent covers both CLIs — don't start duplicates.** A single `csm-agent`
process tails Codex's rollout files *and* reads Claude's event bus, publishing
both to the one topic. You don't need a second one. Check how many are running:

```bash
pgrep -af csm-agent      # expect exactly one line
```

Duplicates just send the same events twice (the desktop de-dupes by session, so
it's harmless but wasteful) — `pkill -f csm-agent` and start a single one.

**How to stop / remove.** The agent installs nothing persistent — **Ctrl-C** (or
`pkill -f csm-agent`) to stop. To remove the Claude hooks it added:

```bash
bash remote-agent.sh --uninstall        # remove the Claude hooks (backs up first)
```

Then delete `remote-agent.sh`. Locally, clear the topic in **Settings → Remote**
to stop subscribing.

> **Per-user, not system-wide.** Everything is scoped to the user who runs the
> command — it edits that user's `~/.claude` / reads their `~/.codex`, nothing
> global. On the remote, run as the user whose CLIs you want to watch (SSH in as
> them, or `sudo -u <user> bash remote-agent.sh --install-claude`); run one agent
> per user to watch several.

> **Privacy:** a public `ntfy.sh` topic is readable by anyone who knows it. Use a
> hard-to-guess topic and/or a self-hosted ntfy + token. Only metadata (source,
> host, directory, status, timestamps) is published — never conversation content.

### What is the "access token"?

It's **optional** — only needed if you lock your topic down so it isn't
world-readable. ntfy uses it as a **Bearer credential**, sent on both *publish*
(the agent) and *subscribe* (the app); with it set, only holders of the token can
read or post your session metadata. Leave it empty for an open public topic. To
protect a topic:

- **ntfy.sh:** sign up, **reserve** the topic and set it to deny anonymous access,
  then generate an **access token** (Account → Access tokens) and paste it here.
- **Self-hosted ntfy:** enable auth and create a user + token with an ACL granting
  read/write on the topic.

---

## Working over Remote-SSH

This is the common case of *"I edit a remote folder in VS Code/Cursor via the
Remote-SSH extension, and run Claude/Codex in that window's terminal."* Two things
are happening, and it helps to separate them:

- **The session runs on the remote** (its terminal is on the server). To **see its
  status**, set up **Remote monitoring** above — run `csm-agent` on that server.
  The card then shows the session with the remote host's name.
- **The editor window runs locally** (Remote-SSH only proxies the files; the
  window is on your Windows desktop). So **clicking the card still focuses that
  window** — jump and the active-window highlight work just like a local session.

So a full Remote-SSH setup is: **`csm-agent` on the server (for status)** +
**clicking the card to focus your local Remote-SSH window (for jump)**. If the
jump is fuzzy, run **Optimize jump** once (above) — it works for Remote-SSH titles
too.

> If you *only* want to focus the window and don't need live status, you don't even
> need the agent — but then the card won't appear until the relay reports it.

---

## System tray

Right-click the tray icon:

- **Show / Hide** — toggle the panel.
- **Reset ball position** — recover the widget if it ended up off-screen.
- **Quit** — fully exit. (Closing the window just hides to the tray.)

---

## Quick reference

| Want to… | Do this |
| --- | --- |
| Jump to a session's editor window | **Click the card** |
| Close a card (until it acts again) | Click its **×** |
| Collapse the panel to a floating ball | Click the **title bar** (Lightweight on) |
| Pop a docked bar back to a ball | **Hover** it |
| Reopen the full panel | **Click** the ball |
| Move it to another edge | **Drag** the ball / bar there |
| Recover a widget that's off-screen | Tray → **Reset ball position** |
| Show / hide · quit | **Tray** menu |
| Tell same-named folders apart | Settings → **Optimize jump** |
| See a remote machine's sessions | Settings → **Remote** (run `csm-agent` there) |
| Switch language | Settings → **Language** |

| Environment variable | Effect |
| --- | --- |
| `CSM_DEMO=1` | Run on self-contained synthetic data (for demos / screenshots) |
| `CSM_DEBUG=1` | Write diagnostics to `~/.cli-session-monitor/csm.log` (off by default) |

---

## Troubleshooting

- **A card says "No matching window".** The editor for that folder isn't open, or
  two folders share a name — open it, or run **Optimize jump**.
- **The wrong / no card is highlighted.** Same cause; **Optimize jump** makes the
  match exact. Idle sessions are never highlighted.
- **No notifications.** Check **Settings → Desktop notification on completion**,
  and Windows' notification settings for the app.
- **SmartScreen warned on first launch.** The release isn't code-signed yet —
  *More info → Run anyway*. (Tracked in `docs/POLISH.md`.)
- **Need diagnostics?** Launch with `CSM_DEBUG=1`; it writes
  `~/.cli-session-monitor/csm.log`. Off by default.
