# Security & Privacy

## Reporting a vulnerability

Please **don't** open a public issue for security problems. Instead, use GitHub's
**private vulnerability reporting** (repo → Security → "Report a vulnerability"),
or open a minimal issue asking for a private contact. We'll respond as soon as we
can.

## Data handling

- **Metadata only.** The tool reads/transmits only session metadata — source,
  host, working directory, status, timestamps. It **never reads or sends your
  conversation content.**
- **Local by default.** Nothing leaves your machine unless you enable the remote
  relay.
- **Remote relay (opt-in).** Events are published to the [ntfy](https://ntfy.sh)
  topic you configure. Public `ntfy.sh` topics are readable by anyone who knows
  the topic name — use a hard-to-guess topic and/or a **self-hosted ntfy with an
  access token** for anything sensitive.
- **Config edits** to other tools (`~/.claude/settings.json`, VS Code/Cursor
  settings) are append-only, backed up first, idempotent, and reversible.
- **Process inspection (Windows).** To tell whether a CLI is actually open, the
  app reads the working directory of running `codex.exe` / `claude.exe`
  processes (your own, same-user). It does not read their memory beyond that.
