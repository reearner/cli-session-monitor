# Polish & UX audit (maintainer notes)

A review of friction points from the angle of shipping this to a broad
open-source audience, with proposed fixes and a rough priority. Implemented items
are checked; the rest is the roadmap.

## P0 — Internationalization (in progress)

**Problem.** The README, docs, and code comments are English, but the **UI is
hardcoded Chinese** (`运行中`, `等待输入`, settings labels, tray menu, desktop
notifications). For an English-documented project this is the single biggest
barrier for non-Chinese users — and equally, forcing English on a Chinese user
would be wrong. The app should speak the user's language.

**Design.**

- A `language` config field: `"auto" | "en" | "zh"`, default **`auto`**.
  - `auto` resolves to `zh` when the OS/browser locale is Chinese, else `en`.
    Frontend uses `navigator.language`; the Rust side uses
    `GetUserDefaultUILanguage` (Windows), falling back to `en`.
- **Frontend**: a tiny `src/i18n.ts` exposing `t(key, vars?)` over an `en`/`zh`
  string table; `setLang(pref)` resolves and caches the active language. All
  user-facing strings in `session-card.ts`, `main.ts`, `settings.ts`, and the
  static `index.html` go through it. A **language selector** in Settings switches
  live (re-renders cards + settings via a `csm:lang` event).
- **Rust**: `notify.rs` (completion / waiting toasts) and the tray menu localize
  off `config.language` with the same `auto` resolution.

**Phase 2 — done.** The operational result strings returned by
`optimize_editor_jump` / `editor_jump_status` / `revert_editor_jump` and the
relay-export command, plus the install/uninstall result text, now localize too:
the editor-jump commands take `State` and route through `i18n::resolve`, and the
install/uninstall result is rebuilt on the frontend from the structured
`InstallOutcome` fields (so it follows the UI language without a Rust round-trip).

## P1 — First-run onboarding

**Partly done.** The empty-state hint is now a clear call-to-action and is
**clickable — it opens Settings** so new users can enable Claude Code without
hunting for the gear. Still open: auto-opening Settings on the first ever launch.

## P1 — Distribution & trust

- The release binary is **unsigned**, so Windows SmartScreen warns on first run.
  The "More info → Run anyway" step is documented in the README. **SHA-256
  checksums are now published** with each release (`SHA256SUMS.txt`, via
  `release.yml`). Still open: signing the installer (or shipping via a package
  manager).
- Consider the **Tauri updater** for in-app updates so users aren't stuck on old
  versions.

## P2 — Accessibility & motion

- ~~Honor `prefers-reduced-motion`: the orb's pulse and the completion flash
  should fall back to a static state.~~ **Done** — animations are disabled under
  `prefers-reduced-motion`; status still shows via color/label/notifications.
- Keyboard: the panel is mouse-driven. At minimum, make cards focusable and
  Enter-activatable, and ensure visible focus rings.
- Contrast: verify the muted text (`--muted` on `--panel`) meets WCAG AA.

## P2 — Robustness & feedback

- **Jump failure feedback.** Clicking a card whose window can't be found is
  silent; surface a brief toast/inline hint ("window not found").
- **Many sessions.** The panel scrolls; confirm the scroll affordance is obvious
  and the floating bar's counts stay readable with large numbers.
- **Idle threshold default** (currently 2 min in defaults, 7 h in the running
  config) — pick one sensible documented default.

## P3 — Settings UX

- Group with clearer section intros and inline help; the **Optimize-jump** option
  is important for accurate jump/highlight but easy to miss.
- Add a "test notification" button and a relay "connection OK" indicator.

## Done (for reference)

- Closable cards; active-window highlight (host-aware, Remote-SSH); demo mode
  (`CSM_DEMO`); diagnostic logging gate (`CSM_DEBUG`); flash/dock fixes;
  open-source docs (README/CONTRIBUTING/SECURITY/CoC/CI/issue templates).
- **i18n P0 + Phase 2** (UI, tray, notifications, install/jump/export results);
  **reduced-motion** support; **clickable empty-state** onboarding; **release
  checksums** (`SHA256SUMS.txt`); README polished to high-star conventions.
