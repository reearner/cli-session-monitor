import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { t } from "./i18n";

/**
 * On launch, check for an update; if one is available, show a small banner with
 * an Install button. Fail-safe — any error (no/placeholder updater key yet, no
 * network, not running under Tauri) is swallowed and simply shows nothing.
 */
export async function checkForUpdate(): Promise<void> {
  let update;
  try {
    update = await check();
  } catch {
    return; // no updater configured / offline / not in Tauri — stay quiet
  }
  if (!update) return;

  const bar = document.createElement("div");
  bar.className = "update-bar";

  const text = document.createElement("span");
  text.className = "update-text";
  text.textContent = t("update.available", { version: update.version });

  const install = document.createElement("button");
  install.className = "btn";
  install.textContent = t("update.install");
  install.addEventListener("click", async () => {
    install.disabled = true;
    install.textContent = t("update.installing");
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch {
      install.disabled = false;
      install.textContent = t("update.install");
      text.textContent = t("update.failed");
    }
  });

  const later = document.createElement("button");
  later.className = "btn ghost";
  later.textContent = "×";
  later.title = t("update.later");
  later.addEventListener("click", () => bar.remove());

  bar.append(text, install, later);
  document.body.append(bar);
}
