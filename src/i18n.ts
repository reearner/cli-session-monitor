// Tiny i18n: `t(key, vars?)` over an en/zh table. The active language comes from
// the `language` config ("auto" | "en" | "zh"); "auto" follows the browser/OS
// locale. Mirrors src-tauri/src/i18n.rs (tray + notifications).

export type Lang = "en" | "zh";

let lang: Lang = "en";

/** Resolve the config preference to a concrete language and cache it. */
export function setLang(pref: string | undefined | null): void {
  if (pref === "en" || pref === "zh") {
    lang = pref;
    return;
  }
  // "auto" / unknown -> follow the locale.
  lang = (navigator.language || "").toLowerCase().startsWith("zh") ? "zh" : "en";
}

export function currentLang(): Lang {
  return lang;
}

type Entry = { en: string; zh: string };

const STR: Record<string, Entry> = {
  // ---- session card ----
  "status.running": { en: "Running", zh: "运行中" },
  "status.done": { en: "Replied", zh: "已回复" },
  "status.waiting": { en: "Needs your input", zh: "等待你确认" },
  "status.idle": { en: "Idle", zh: "空闲" },
  "card.close": {
    en: "Close this card (reappears when the session acts again)",
    zh: "关闭此卡片（会话再次活动时会重新出现）",
  },
  "card.copyResume": {
    en: "Click: copy the resume command · Right-click: edit & remember it (e.g. add --yolo)",
    zh: "左键：复制恢复命令 · 右键：编辑并记住（如加 --yolo）",
  },
  "card.copyResumeDir": {
    en: "Click: copy a command to resume the most recent session in this folder · Right-click: edit & remember it",
    zh: "左键：复制命令（恢复该文件夹最近一次会话）· 右键：编辑并记住",
  },
  "card.rename": {
    en: "Rename this card (kept for this session, even after resuming it)",
    zh: "重命名此卡片（按会话保留，恢复该会话后仍显示）",
  },
  "card.launchDir": { en: "Launch dir: {dir}", zh: "启动目录：{dir}" },
  "card.estimate": { en: "est.", zh: "估算" },
  "card.clickRemote": {
    en: "Click to switch to its editor window (if opened locally via Remote-SSH)",
    zh: "点击切换到对应编辑器窗口（若通过 Remote-SSH 在本机打开）",
  },
  "card.clickLocal": { en: "Click: focus / open the editor — {dir}", zh: "点击：聚焦/打开对应编辑器 — {dir}" },
  "card.window": { en: "Window: {title}", zh: "对应窗口：{title}" },
  "card.noWindow": { en: "🪟 No matching window", zh: "🪟 未找到对应窗口" },
  "card.noWindowTitle": { en: "No matching editor window", zh: "没有匹配的编辑器窗口" },

  // ---- auto-update ----
  "update.available": { en: "Update {version} available", zh: "有新版本 {version}" },
  "update.install": { en: "Install & restart", zh: "安装并重启" },
  "update.installing": { en: "Installing…", zh: "安装中…" },
  "update.later": { en: "Later", zh: "稍后" },
  "update.failed": { en: "Update failed", zh: "更新失败" },

  // ---- top bar / empty ----
  "app.settings": { en: "Settings", zh: "设置 / 接入" },
  "app.empty": {
    en: "No active sessions yet. Click here to open Settings and enable Claude Code (Codex needs nothing) — sessions then show up live.",
    zh: "暂无活动会话。点这里打开设置、启用 Claude Code(Codex 无需设置)——之后会话会实时显示。",
  },

  // ---- settings ----
  "set.title": { en: "Settings", zh: "设置" },
  "set.language": { en: "Language", zh: "语言" },
  "set.lang.auto": { en: "Auto", zh: "自动" },
  "set.notify": { en: "Desktop notification on completion", zh: "完成时桌面通知" },
  "set.sound": { en: "Sound on completion", zh: "完成时提示音" },
  "set.onTop": { en: "Always on top", zh: "窗口置顶" },
  "set.lightweight": {
    en: "Lightweight (collapse to a ball; flashes on completion / waiting)",
    zh: "轻量模式（缩成小胶囊，完成/待处理时闪烁）",
  },
  "set.keepMinutes": { en: "Keep finished for (minutes)", zh: "完成后保留（分钟）" },
  "set.keepDays": { en: "Keep idle sessions for", zh: "空闲会话保留" },
  "set.daysUnit": { en: "days", zh: "天" },
  "set.resident": { en: "Desktop-resident", zh: "桌面常驻" },
  "set.autostart": { en: "Start on login", zh: "开机自启" },
  "set.dockLeft": { en: "Dock to left edge", zh: "停靠屏幕左侧" },
  "set.pinned": { en: "Pin to desktop (not on top)", zh: "贴桌面层（不置顶）" },
  "set.skipTaskbar": { en: "Hide from taskbar (tray only)", zh: "不在任务栏显示（仅托盘）" },

  "set.integrations": { en: "Integrations", zh: "接入" },
  "set.conflict": { en: "Conflict: {msg}", zh: "冲突：{msg}" },
  "set.outcome.installed": { en: "Installed ✓ — reporting hooks written.", zh: "已接入 ✓ —— 已写入上报 hook。" },
  "set.outcome.uninstalled": { en: "Uninstalled ✓ — our hooks removed.", zh: "已卸载 ✓ —— 已移除本应用的 hook。" },
  "set.outcome.alreadyInstalled": { en: "Already installed — no changes.", zh: "已接入,无改动。" },
  "set.outcome.alreadyClean": { en: "Nothing to remove.", zh: "无可移除项。" },
  "set.outcome.backup": { en: "Backup: {path}", zh: "备份:{path}" },
  "set.claudeNote": {
    en: "Claude Code: writes a reporting hook into settings.json (append-only, backed up first, one-click uninstall).",
    zh: "Claude Code：把上报 hook 写入 settings.json（只追加、写前备份、可一键卸载）。",
  },
  "set.checking": { en: "Checking…", zh: "检测中…" },
  "set.install": { en: "Install", zh: "安装接入" },
  "set.installFailed": { en: "Install failed: {err}", zh: "安装失败：{err}" },
  "set.uninstall": { en: "Uninstall", zh: "卸载" },
  "set.uninstallFailed": { en: "Uninstall failed: {err}", zh: "卸载失败：{err}" },
  "set.codexNote": {
    en: "Codex: monitored automatically (reads ~/.codex session files) — no install needed.",
    zh: "Codex：自动监视（读取 ~/.codex 会话文件），无需安装。",
  },
  "set.autoMonitored": { en: "Auto-monitored ✓", zh: "自动监视 ✓" },
  "set.removeOldNotify": { en: "Remove old notify", zh: "卸载旧 notify" },
  "set.installed": { en: "Installed ✓", zh: "已接入 ✓" },
  "set.notInstalled": { en: "Not installed", zh: "未接入" },
  "set.statusUnknown": { en: "Status unknown", zh: "状态未知" },

  "set.jump": { en: "Editor jump", zh: "编辑器跳转" },
  "set.jumpNote": {
    en: "Click a session card to jump to its editor window. Works out of the box in most cases; only when two windows open same-named folders do you need the full path in the title to tell them apart. One click writes it into VS Code/Cursor settings (backed up, won't override existing, reversible).",
    zh: "点击会话卡片可跳到对应编辑器窗口。多数情况开箱即用；仅当两个窗口打开的文件夹同名时，需把完整路径写进窗口标题来区分。一键写入 VS Code/Cursor 设置（备份、不覆盖已有、可还原）。",
  },
  "set.configured": { en: "Configured ✓", zh: "已配置 ✓" },
  "set.notConfigured": { en: "Not configured", zh: "未配置" },
  "set.optimize": { en: "Optimize jump", zh: "优化跳转" },
  "set.optimizeFailed": { en: "Optimize failed: {err}", zh: "优化失败：{err}" },
  "set.revert": { en: "Revert", zh: "还原" },
  "set.revertFailed": { en: "Revert failed: {err}", zh: "还原失败：{err}" },

  "set.testNotify": { en: "Send a test notification", zh: "发送测试通知" },
  "set.relayState": { en: "Relay", zh: "中继" },
  "set.relayOff": { en: "Off (no topic)", zh: "未启用（无主题）" },
  "set.relayConnecting": { en: "Connecting…", zh: "连接中…" },
  "set.relayConnected": { en: "Connected ✓", zh: "已连接 ✓" },

  "set.remote": { en: "Remote (relay)", zh: "远程（中继）" },
  "set.remoteNote": {
    en: "Run csm-agent on a remote server publishing to the same topic; subscribe here to see its sessions. Changes apply live (auto-reconnect, replaying the last 5 minutes).",
    zh: "在远端服务器运行 csm-agent 并发布到同一个主题，这里订阅即可看到远端会话。改动即时生效（自动重连，并补取最近 5 分钟事件）。",
  },
  "set.topic": { en: "Subscribe topic", zh: "订阅主题" },
  "set.topicPlaceholder": { en: "empty = remote off", zh: "留空=关闭远程" },
  "set.relayUrl": { en: "Relay URL", zh: "中继地址" },
  "set.token": { en: "Access token (optional)", zh: "访问令牌(可选)" },
  "set.exportScript": { en: "Export remote script", zh: "导出远端脚本" },
  "set.copy": { en: "Copy", zh: "复制" },
  "set.copied": { en: "Copied ✓", zh: "已复制 ✓" },
  "set.remoteScript": { en: "Remote script", zh: "远端脚本" },
  "set.exportFailed": { en: "Export failed: {err}", zh: "导出失败：{err}" },
  "set.exportOk": {
    en: "Exported: {path}\n① Copy it to the remote and run: bash remote-agent.sh\n   Codex is automatic; for Claude run: bash remote-agent.sh --install-claude\n   (remove later with: bash remote-agent.sh --uninstall)\n② Subscribed to topic: {topic} — the remote session appears once its agent is running.",
    zh: "已导出：{path}\n① 拷到远端并运行：bash remote-agent.sh\n   Codex 自动纳入;监控 Claude 运行:bash remote-agent.sh --install-claude\n   (以后卸载:bash remote-agent.sh --uninstall)\n② 已订阅主题:{topic} —— 远端 agent 跑起来后,该会话就会出现。",
  },
};

/** Translate `key`, substituting `{name}` placeholders from `vars`. */
export function t(key: string, vars?: Record<string, string>): string {
  let s = STR[key]?.[lang] ?? key;
  if (vars) {
    for (const k in vars) s = s.replaceAll(`{${k}}`, vars[k]);
  }
  return s;
}
