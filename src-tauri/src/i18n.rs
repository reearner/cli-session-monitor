//! Minimal localization for the Rust-side user-facing strings (tray menu,
//! desktop notifications, and the editor-jump / export command results shown in
//! Settings). Mirrors `src/i18n.ts`; `auto` resolves against the OS UI language.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
}

/// Resolve a `language` config value ("auto" | "en" | "zh") to a concrete lang.
pub fn resolve(pref: &str) -> Lang {
    match pref {
        "en" => Lang::En,
        "zh" => Lang::Zh,
        _ => detect(),
    }
}

#[cfg(windows)]
fn detect() -> Lang {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    // LANGID: the low 10 bits are the primary language id; 0x04 == Chinese.
    let langid = unsafe { GetUserDefaultUILanguage() };
    if (langid & 0x3ff) == 0x04 {
        Lang::Zh
    } else {
        Lang::En
    }
}

#[cfg(not(windows))]
fn detect() -> Lang {
    Lang::En
}

impl Lang {
    // ---- tray menu ----
    pub fn tray_toggle(self) -> &'static str {
        match self {
            Lang::En => "Show / Hide",
            Lang::Zh => "显示 / 隐藏",
        }
    }
    pub fn tray_reset(self) -> &'static str {
        match self {
            Lang::En => "Reset ball position",
            Lang::Zh => "重置悬浮球位置",
        }
    }
    pub fn tray_quit(self) -> &'static str {
        match self {
            Lang::En => "Quit",
            Lang::Zh => "退出",
        }
    }

    // ---- notifications ----
    /// "{source} finished" / "{source} 已完成"
    pub fn notify_finished_title(self, source: &str) -> String {
        match self {
            Lang::En => format!("{source} finished"),
            Lang::Zh => format!("{source} 已完成"),
        }
    }
    pub fn notify_finished_title_unknown(self) -> String {
        match self {
            Lang::En => "Session finished".into(),
            Lang::Zh => "会话已完成".into(),
        }
    }
    /// "{source} needs you" / "{source} 等待你的操作"
    pub fn notify_waiting_title(self, source: &str) -> String {
        match self {
            Lang::En => format!("{source} needs you"),
            Lang::Zh => format!("{source} 等待你的操作"),
        }
    }
    /// Body: a "waiting for input" line, optionally suffixed with the directory.
    pub fn notify_waiting_body(self, cwd: &str) -> String {
        let suffix = if cwd.is_empty() {
            String::new()
        } else {
            format!("  ·  {cwd}")
        };
        match self {
            Lang::En => format!("Waiting for your input{suffix}"),
            Lang::Zh => format!("需要你确认/输入{suffix}"),
        }
    }

    // ---- editor-jump command results (Settings) ----
    pub fn jump_only_windows(self) -> &'static str {
        match self {
            Lang::En => "Only VS Code / Cursor on Windows is supported.",
            Lang::Zh => "仅支持 Windows 上的 VS Code / Cursor。",
        }
    }
    pub fn jump_no_settings(self) -> &'static str {
        match self {
            Lang::En => "No VS Code / Cursor user settings found.",
            Lang::Zh => "未检测到 VS Code / Cursor 的用户设置。",
        }
    }
    pub fn jump_read_failed(self, name: &str, e: &str) -> String {
        match self {
            Lang::En => format!("{name}: read failed ({e})"),
            Lang::Zh => format!("{name}：读取失败（{e}）"),
        }
    }
    pub fn jump_has_title(self, name: &str) -> String {
        match self {
            Lang::En => format!("{name}: already has a window.title — left unchanged"),
            Lang::Zh => format!("{name}：已有 window.title 设置，未改动"),
        }
    }
    pub fn jump_backup_failed(self, name: &str) -> String {
        match self {
            Lang::En => format!("{name}: backup failed — skipped"),
            Lang::Zh => format!("{name}：备份失败，跳过"),
        }
    }
    pub fn jump_written(self, name: &str, backup: &str) -> String {
        match self {
            Lang::En => format!("{name}: written (backup {backup})"),
            Lang::Zh => format!("{name}：已写入（备份 {backup}）"),
        }
    }
    pub fn jump_write_failed(self, name: &str, e: &str) -> String {
        match self {
            Lang::En => format!("{name}: write failed ({e})"),
            Lang::Zh => format!("{name}：写入失败（{e}）"),
        }
    }
    pub fn jump_status_configured(self, name: &str) -> String {
        match self {
            Lang::En => format!("{name}: configured"),
            Lang::Zh => format!("{name}：已配置"),
        }
    }
    pub fn jump_status_custom(self, name: &str) -> String {
        match self {
            Lang::En => format!("{name}: custom title (add ${{folderPath}})"),
            Lang::Zh => format!("{name}：自定义标题（建议含 ${{folderPath}}）"),
        }
    }
    pub fn jump_status_unconfigured(self, name: &str) -> String {
        match self {
            Lang::En => format!("{name}: not configured"),
            Lang::Zh => format!("{name}：未配置"),
        }
    }
    pub fn jump_none_detected(self) -> &'static str {
        match self {
            Lang::En => "No VS Code / Cursor detected.",
            Lang::Zh => "未检测到 VS Code / Cursor。",
        }
    }
    pub fn revert_nothing(self, name: &str) -> String {
        match self {
            Lang::En => format!("{name}: nothing of ours to remove"),
            Lang::Zh => format!("{name}：无本应用写入的设置"),
        }
    }
    pub fn revert_done(self, name: &str) -> String {
        match self {
            Lang::En => format!("{name}: reverted"),
            Lang::Zh => format!("{name}：已还原"),
        }
    }
    pub fn revert_failed(self, name: &str, e: &str) -> String {
        match self {
            Lang::En => format!("{name}: revert failed ({e})"),
            Lang::Zh => format!("{name}：还原失败（{e}）"),
        }
    }
    pub fn export_need_topic(self) -> &'static str {
        match self {
            Lang::En => "Set a subscribe topic first (Export auto-generates one).",
            Lang::Zh => "请先设置订阅主题（导出时会自动生成）。",
        }
    }
    /// Title + body for the "test notification" button.
    pub fn notify_test(self) -> (String, String) {
        match self {
            Lang::En => (
                "CLI Session Monitor".into(),
                "Test notification — notifications are working.".into(),
            ),
            Lang::Zh => ("CLI Session Monitor".into(), "测试通知 —— 通知功能正常。".into()),
        }
    }
}
