import type { Config, InstallOutcome } from "./types";
import {
  getConfig,
  setConfig,
  integrationStatus,
  installIntegration,
  uninstallIntegration,
  exportAgentScript,
  relayStatus,
  testNotification,
  optimizeEditorJump,
  revertEditorJump,
  editorJumpStatus,
} from "./ipc";
import { t, setLang } from "./i18n";

function el(tag: string, cls?: string, text?: string): HTMLElement {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

function toggleRow(
  label: string,
  checked: boolean,
  onChange: (v: boolean) => void,
): HTMLElement {
  const row = el("label", "row");
  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = checked;
  input.addEventListener("change", () => onChange(input.checked));
  row.append(input, el("span", undefined, label));
  return row;
}

function inputRow(
  label: string,
  value: string,
  placeholder: string,
  onChange: (v: string) => void,
): HTMLElement {
  const row = el("div", "row");
  const lab = el("span", undefined, label);
  const input = document.createElement("input");
  input.type = "text";
  input.value = value;
  input.placeholder = placeholder;
  input.className = "text-input";
  input.addEventListener("change", () => onChange(input.value.trim()));
  row.append(lab, input);
  return row;
}

function selectRow(
  label: string,
  value: string,
  options: { value: string; label: string }[],
  onChange: (v: string) => void,
): HTMLElement {
  const row = el("div", "row");
  const lab = el("span", undefined, label);
  const sel = document.createElement("select");
  sel.className = "text-input";
  for (const o of options) {
    const opt = document.createElement("option");
    opt.value = o.value;
    opt.textContent = o.label;
    if (o.value === value) opt.selected = true;
    sel.append(opt);
  }
  sel.addEventListener("change", () => onChange(sel.value));
  row.append(lab, sel);
  return row;
}

/** Render the settings + integration panel into `container`. */
export async function renderSettings(container: HTMLElement): Promise<void> {
  container.replaceChildren();
  container.append(el("h2", undefined, t("set.title")));

  let cfg: Config;
  try {
    cfg = await getConfig();
  } catch {
    cfg = {
      notifications: true,
      sound: true,
      idle_threshold_secs: 120,
      always_on_top: true,
      autostart: false,
      dock_left: false,
      desktop_pinned: false,
      skip_taskbar: false,
      lightweight: false,
      relay_url: "https://ntfy.sh",
      relay_topic: "",
      relay_token: "",
      win_x: null,
      win_y: null,
      language: "auto",
      panel_w: 360,
      panel_h: 640,
      onboarded: true,
    };
  }

  const persist = async () => {
    try {
      await setConfig(cfg);
    } catch (e) {
      console.error("setConfig failed", e);
    }
  };

  container.append(
    selectRow(
      t("set.language"),
      cfg.language || "auto",
      [
        { value: "auto", label: t("set.lang.auto") },
        { value: "en", label: "English" },
        { value: "zh", label: "中文" },
      ],
      (v) => {
        cfg.language = v;
        void persist();
        setLang(v); // re-render this panel in the new language
        window.dispatchEvent(new CustomEvent("csm:lang", { detail: v }));
        void renderSettings(container);
      },
    ),
    toggleRow(t("set.notify"), cfg.notifications, (v) => {
      cfg.notifications = v;
      void persist();
    }),
    toggleRow(t("set.sound"), cfg.sound, (v) => {
      cfg.sound = v;
      void persist();
    }),
    toggleRow(t("set.onTop"), cfg.always_on_top, (v) => {
      cfg.always_on_top = v;
      void persist();
    }),
    toggleRow(t("set.lightweight"), cfg.lightweight, (v) => {
      cfg.lightweight = v;
      void persist();
      window.dispatchEvent(new CustomEvent("csm:lightweight", { detail: v }));
    }),
    inputRow(
      t("set.keepMinutes"),
      String(Math.round(cfg.idle_threshold_secs / 60)),
      "120",
      (v) => {
        const m = parseInt(v, 10);
        if (Number.isFinite(m) && m >= 0) {
          cfg.idle_threshold_secs = m * 60;
          void persist();
        }
      },
    ),
  );

  // ---- Desktop-resident (all default off) ----
  container.append(el("h2", undefined, t("set.resident")));
  container.append(
    toggleRow(t("set.autostart"), cfg.autostart, (v) => {
      cfg.autostart = v;
      void persist();
    }),
    toggleRow(t("set.dockLeft"), cfg.dock_left, (v) => {
      cfg.dock_left = v;
      void persist();
    }),
    toggleRow(t("set.pinned"), cfg.desktop_pinned, (v) => {
      cfg.desktop_pinned = v;
      void persist();
    }),
    toggleRow(t("set.skipTaskbar"), cfg.skip_taskbar, (v) => {
      cfg.skip_taskbar = v;
      void persist();
    }),
  );

  // Quick check that desktop notifications actually reach you.
  {
    const row = el("div", "integration");
    const btn = el("button", "btn ghost", t("set.testNotify")) as HTMLButtonElement;
    btn.addEventListener("click", () => void testNotification());
    row.append(el("span", "int-name", "🔔"), btn);
    container.append(row);
  }

  // ---- Integration ----
  container.append(el("h2", undefined, t("set.integrations")));

  const result = el("pre", "result");
  // Build a localized result from the structured outcome fields (the Rust-side
  // `summary` is not localized; we render from changed/installed/conflict/backup).
  const show = (o: InstallOutcome) => {
    let msg: string;
    if (o.conflict) {
      msg = t("set.conflict", { msg: o.conflict });
    } else if (!o.changed) {
      msg = o.installed ? t("set.outcome.alreadyInstalled") : t("set.outcome.alreadyClean");
    } else {
      msg = o.installed ? t("set.outcome.installed") : t("set.outcome.uninstalled");
      if (o.backup_path) msg += "\n" + t("set.outcome.backup", { path: o.backup_path });
    }
    result.textContent = msg;
  };

  // Claude Code needs hooks installed into settings.json.
  container.append(el("p", "note", t("set.claudeNote")));
  const claudeTag = el("span", "ok-tag", t("set.checking"));
  {
    const box = el("div", "integration");
    box.append(el("span", "int-name", "Claude Code"), claudeTag);
    const btnInstall = el("button", "btn", t("set.install")) as HTMLButtonElement;
    btnInstall.addEventListener("click", async () => {
      try {
        show(await installIntegration("claude"));
        await refreshStatus();
      } catch (e) {
        result.textContent = t("set.installFailed", { err: String(e) });
      }
    });
    const btnUninstall = el("button", "btn ghost", t("set.uninstall")) as HTMLButtonElement;
    btnUninstall.addEventListener("click", async () => {
      try {
        show(await uninstallIntegration("claude"));
        await refreshStatus();
      } catch (e) {
        result.textContent = t("set.uninstallFailed", { err: String(e) });
      }
    });
    box.append(btnInstall, btnUninstall);
    container.append(box);
  }

  // Codex is monitored automatically via its rollout files — no install needed.
  // (Installing the old notify would double-count completions.)
  container.append(el("p", "note", t("set.codexNote")));
  {
    const box = el("div", "integration");
    box.append(el("span", "int-name", "Codex"));
    box.append(el("span", "ok-tag", t("set.autoMonitored")));
    const btnUninstall = el("button", "btn ghost", t("set.removeOldNotify")) as HTMLButtonElement;
    btnUninstall.addEventListener("click", async () => {
      try {
        show(await uninstallIntegration("codex"));
      } catch (e) {
        result.textContent = t("set.uninstallFailed", { err: String(e) });
      }
    });
    box.append(btnUninstall);
    container.append(box);
  }

  container.append(result);

  // Reflect current status on the Claude row (live, also after install/uninstall).
  async function refreshStatus(): Promise<void> {
    try {
      const st = await integrationStatus();
      claudeTag.textContent = st.claude.installed ? t("set.installed") : t("set.notInstalled");
      claudeTag.className = st.claude.installed ? "ok-tag" : "ok-tag off";
    } catch {
      claudeTag.textContent = t("set.statusUnknown");
      claudeTag.className = "ok-tag off";
    }
  }
  await refreshStatus();

  // ---- Editor jump optimization (only needed for same-named folders) ----
  container.append(el("h2", undefined, t("set.jump")));
  container.append(el("p", "note", t("set.jumpNote")));
  const jumpTag = el("span", "ok-tag", t("set.checking"));
  async function refreshJump(): Promise<void> {
    try {
      const st = await editorJumpStatus();
      jumpTag.textContent = st.configured ? t("set.configured") : t("set.notConfigured");
      jumpTag.className = st.configured ? "ok-tag" : "ok-tag off";
      jumpTag.title = st.summary;
    } catch {
      jumpTag.textContent = t("set.statusUnknown");
      jumpTag.className = "ok-tag off";
    }
  }
  {
    const box = el("div", "integration");
    box.append(el("span", "int-name", "VS Code / Cursor"), jumpTag);
    const btnOpt = el("button", "btn", t("set.optimize")) as HTMLButtonElement;
    btnOpt.addEventListener("click", async () => {
      try {
        result.textContent = await optimizeEditorJump();
        await refreshJump();
      } catch (e) {
        result.textContent = t("set.optimizeFailed", { err: String(e) });
      }
    });
    const btnRevert = el("button", "btn ghost", t("set.revert")) as HTMLButtonElement;
    btnRevert.addEventListener("click", async () => {
      try {
        result.textContent = await revertEditorJump();
        await refreshJump();
      } catch (e) {
        result.textContent = t("set.revertFailed", { err: String(e) });
      }
    });
    box.append(btnOpt, btnRevert);
    container.append(box);
  }
  await refreshJump();

  // ---- Remote relay (ntfy): see sessions from a remote server, no SSH ----
  container.append(el("h2", undefined, t("set.remote")));
  container.append(el("p", "note", t("set.remoteNote")));
  // Live status: is a topic set, and is the subscription stream actually open?
  {
    const row = el("div", "integration");
    const tag = el("span", "ok-tag", t("set.checking"));
    row.append(el("span", "int-name", t("set.relayState")), tag);
    container.append(row);
    const refreshRelay = async () => {
      try {
        const st = await relayStatus();
        if (!st.subscribed) {
          tag.textContent = t("set.relayOff");
          tag.className = "ok-tag off";
        } else {
          tag.textContent = st.connected ? t("set.relayConnected") : t("set.relayConnecting");
          tag.className = st.connected ? "ok-tag" : "ok-tag off";
        }
      } catch {
        tag.textContent = t("set.statusUnknown");
        tag.className = "ok-tag off";
      }
    };
    void refreshRelay();
    // Re-check a couple of times so a freshly-(re)started subscription flips to
    // "connected" without a persistent polling interval (which would leak on
    // re-render). One-shot timeouts self-clear.
    window.setTimeout(() => void refreshRelay(), 1500);
    window.setTimeout(() => void refreshRelay(), 4000);
  }
  const topicRow = inputRow(t("set.topic"), cfg.relay_topic, t("set.topicPlaceholder"), (v) => {
    cfg.relay_topic = v;
    void persist();
  });
  const topicInput = topicRow.querySelector("input") as HTMLInputElement;
  container.append(
    inputRow(t("set.relayUrl"), cfg.relay_url, "https://ntfy.sh", (v) => {
      cfg.relay_url = v || "https://ntfy.sh";
      void persist();
    }),
    topicRow,
    inputRow(t("set.token"), cfg.relay_token, "ntfy token", (v) => {
      cfg.relay_token = v;
      void persist();
    }),
  );

  // Export a ready-to-run remote launcher script (topic baked in). If no topic
  // is set yet, generate a hard-to-guess one and save it first.
  const relayResult = el("pre", "result");
  const exportBox = el("div", "integration");
  const btnExport = el("button", "btn", t("set.exportScript")) as HTMLButtonElement;
  const btnCopy = el("button", "btn", t("set.copy")) as HTMLButtonElement;
  btnCopy.disabled = true;
  btnExport.addEventListener("click", async () => {
    try {
      if (!cfg.relay_topic.trim()) {
        cfg.relay_topic = `csm-${crypto.randomUUID()}`;
        topicInput.value = cfg.relay_topic;
        await setConfig(cfg);
      }
      const path = await exportAgentScript();
      relayResult.textContent = t("set.exportOk", { path, topic: cfg.relay_topic });
      btnCopy.disabled = false;
    } catch (e) {
      relayResult.textContent = t("set.exportFailed", { err: String(e) });
    }
  });
  // One-click copy of the export result (the global user-select:none otherwise
  // makes the panel awkward to select by hand). Falls back to execCommand if the
  // async clipboard API is blocked in the webview.
  btnCopy.addEventListener("click", async () => {
    const text = relayResult.textContent ?? "";
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const range = document.createRange();
      range.selectNodeContents(relayResult);
      const sel = window.getSelection();
      sel?.removeAllRanges();
      sel?.addRange(range);
      try {
        document.execCommand("copy");
      } catch {
        /* ignore — text is selectable, the user can copy manually */
      }
      sel?.removeAllRanges();
    }
    const label = btnCopy.textContent;
    btnCopy.textContent = t("set.copied");
    setTimeout(() => {
      btnCopy.textContent = label;
    }, 1200);
  });
  exportBox.append(el("span", "int-name", t("set.remoteScript")), btnExport, btnCopy);
  container.append(exportBox, relayResult);
}
