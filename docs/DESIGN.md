# CLI Session Monitor — 设计报告（含跨平台 / Linux 实现指南）

> 版本：0.1.0（MVP 进行中） ｜ 技术栈：Tauri (Rust + Web) + Vanilla TS ｜ License：MIT
> 本报告是面向实现者的完整设计说明，并专门给出 **Linux 端实现指南**。
> 配套规格文档：`.spec-workflow/specs/cli-session-monitor/{requirements,design,tasks}.md`。

---

## 1. 目标与问题本质

一个常驻桌面的轻量挂件，实时显示本机/远端多个 CLI 编程会话（Claude Code、Codex）的运行状态：模型是**正在生成**（`running`，秒表走动）还是**已答完**（`done`，显示本轮耗时），并在完成时发桌面通知/提示音。

**核心难点**：CLI 跑在终端里、进程长期驻留，从外部**看不出**模型是"在思考"还是"已答完"。解决办法是借各 CLI 官方的**确定性生命周期机制**在事件发生的准确时机上报：

- **Claude Code**：hooks（`UserPromptSubmit` / `Stop` / `SessionEnd`），由 harness 在生命周期事件上确定性触发，不依赖模型"记得上报"。
- **Codex**：`notify`（回合完成等），在 `~/.codex/config.toml` 配置一个外部程序，回合结束时被调用并传入一段 JSON。

两条设计主轴：

1. **可靠性**：状态来自 harness 确定性事件；上报链路任何故障都不得阻塞或破坏 CLI。
2. **可插拔传输**：reporter 输出端 `Sink`、App 输入端 `Source` 都是接口。MVP 仅实现"本地文件 / fs 监听"；Phase 2 远端走"网络 sink / 网络订阅"而**不返工**。

---

## 2. 总体架构

三个部件，靠**统一事件 schema** 经**文件总线**解耦：

```mermaid
graph TD
    subgraph CLIs["被监控的 CLI（harness 确定性事件）"]
        CC["Claude Code<br/>UserPromptSubmit / Stop / SessionEnd<br/>(hook → stdin JSON)"]
        CX["Codex<br/>notify 回合完成<br/>(argv JSON)"]
    end

    subgraph Reporter["session-reporter（独立可执行，永远 exit 0）"]
        AD["CLI Adapter (claude / codex)"]
        SINK["Sink 接口"]
        FSINK["FileSink (MVP)：tmp + 原子 rename"]
        NSINK["NetworkSink (Phase 2)"]
        AD --> SINK --> FSINK
        SINK -.Phase2.-> NSINK
    end

    BUS[("~/.cli-session-monitor/events/<br/>JSON 事件文件")]

    subgraph App["Tauri App 后端 (Rust)"]
        SRC["Source 接口"]
        FSRC["FsWatchSource (MVP)：排空+监听+去抖"]
        NSRC["NetworkSource (Phase 2)"]
        SM["StateMachine：running→done→idle / session_end 移除"]
        NOTI["Notifier：桌面通知 + 提示音"]
        TRAY["Tray + Window"]
        INST["Installer：安装/卸载/备份/幂等"]
        FSRC --> SRC --> SM --> NOTI
        NSRC -.Phase2.-> SRC
    end

    subgraph FE["前端 (Vanilla TS)"]
        CARDS["会话卡片列表（按 host 分组）"]
        TIMER["本地 1s 秒表（由 run_started_at 计算）"]
    end

    CC --> AD
    CX --> AD
    FSINK -->|write| BUS
    BUS -->|watch+read+delete| FSRC
    SM -->|emit 快照(变更时)| CARDS --> TIMER
    INST -->|写接入配置| CC
    INST -->|写接入配置| CX
```

**关键设计点**：后端只在状态**变更时** emit 快照；秒表在前端用 `run_started_at` 本地每秒计算，避免每秒 IPC。

---

## 3. 跨平台设计原则与平台抽象层

整套系统**绝大部分是平台无关的纯 Rust 逻辑**；平台差异集中在少数几处。Linux 实现的核心是：**复用全部纯逻辑，只替换/适配平台相关层**。

### 3.1 平台相关点清单（务必集中管理）

| 关注点 | 当前实现 | 跨平台状态 | Linux 处理 |
|--------|----------|------------|------------|
| **数据/事件/配置目录** | `dirs::home_dir().join(".cli-session-monitor")`，在 reporter/config/fs_watch **3 处重复** | ✅ 可跨平台，但重复 | 建议收敛到共享 `paths` 模块；Linux 可选改用 XDG（见 §7.3） |
| **主机名 `host`** | `COMPUTERNAME` → 否则 `HOSTNAME` env | ⚠️ **Linux 不可靠**（非交互 shell 常不导出 `HOSTNAME`） | **改用 `gethostname` crate**（见 §8） |
| **reporter 二进制名** | `session-reporter.exe` | 平台相关后缀 | Linux 为 `session-reporter`（cargo 自动）；安装器写路径时不能硬编码 `.exe` |
| **原子写 temp+rename** | `fs::rename` | ✅ 同盘原子，Linux 一致 | 无需改动 |
| **文件监听** | `notify` crate | ✅ Win=ReadDirectoryChanges，Linux=inotify | 无需改动 |
| **Claude/Codex 配置路径** | `~/.claude/settings.json`、`~/.codex/config.toml` | ✅ 两平台同为 `~` 下 dotfolder | 无需改动；hook 命令里的 reporter 路径不带 `.exe` |
| **桌面通知** | `tauri-plugin-notification` | ✅ Win=WinRT Toast，Linux=`org.freedesktop.Notifications` | 需 DE 支持通知守护（多数自带） |
| **系统托盘** | Tauri tray | ⚠️ Linux 需 AppIndicator | 装 `libayatana-appindicator3`；GNOME 需扩展（见 §7.5） |
| **GUI 运行时** | WebView2（Win 内置） | ⚠️ 平台不同 | Linux=WebKitGTK（`webkit2gtk-4.1`） |
| **工具链/构建** | Win-GNU + Strawberry mingw + ASCII TMP（本机特例） | — | **Linux 原生 gcc/ld，无 mingw/TMP 烦恼**，最简单 |
| **(Phase2) 聚焦终端窗口** | 未做 | 高度平台相关 | Win=SetForegroundWindow；Linux X11=wmctrl，Wayland 受限 |

### 3.2 建议的重构（为跨平台收敛）

把分散的路径/主机名逻辑收敛进 **`csm-core`** 的一个 `paths`/`env` 模块（或一个独立 `csm-platform` crate），使所有平台分支只在一处：

```rust
// csm-core 内（建议新增）
pub fn data_dir() -> PathBuf;       // ~/.cli-session-monitor (或 Linux XDG)
pub fn events_dir() -> PathBuf;     // data_dir()/events
pub fn config_path() -> PathBuf;    // data_dir()/config.json
pub fn host_name() -> String;       // gethostname()，统一两平台
pub fn claude_settings_path() -> PathBuf; // ~/.claude/settings.json
pub fn codex_config_path() -> PathBuf;    // ~/.codex/config.toml
```

这样 reporter 与 App 共用同一套路径定义，Linux 适配只动这一处。

---

## 4. 统一事件 Schema（跨进程唯一契约）

```jsonc
{
  "schema": 1,                 // SCHEMA_VERSION，演进用；消费者据此跳过不兼容
  "source": "claude-code",     // "claude-code" | "codex"
  "session_id": "…",
  "cwd": "/proj",              // 项目目录（远端为远端路径）
  "host": "my-laptop",         // 来源主机/设备标识；本机=机器名，远端=远端 host
  "event": "run_start",        // "run_start" | "run_end" | "session_end"
  "ts": 1700000000000          // epoch 毫秒
}
```

- **会话身份** `SessionKey = (source, host, session_id)`：`host` 是身份的一部分 → 同一 `session_id` 在两台机器上**不冲突**，这正是承载远端/跨设备的关键。`cwd` 不参与身份。
- 事件映射：

| CLI | 触发点 | → EventKind |
|-----|--------|-------------|
| Claude Code | `UserPromptSubmit` | `run_start`（开始计时） |
| Claude Code | `Stop` | `run_end`（停表、算耗时、通知） |
| Claude Code | `SessionEnd` | `session_end`（移除卡片） |
| Codex | `notify`（回合完成） | `run_end`（无干净"开始"信号 → 计时退化，见 §9） |

---

## 5. 组件详解（标注 跨平台 / 平台相关）

### 5.1 `csm-core`（共享契约）— 跨平台
- `SCHEMA_VERSION`、`Source`、`EventKind`、`Event`、`SessionKey`，全部 `serde` 强类型。
- **建议**：把 §3.2 的 `paths`/`host_name` 也放这里，作为唯一平台分支点。

### 5.2 `session-reporter`（上报可执行）— 几乎全跨平台
- **adapter/claude.rs**：从 **stdin** 读 hook JSON（`session_id`/`cwd`/`hook_event_name`）→ 映射 EventKind。
- **adapter/codex.rs**：从 **argv 末参或 stdin** 读 notify JSON → 回合完成映射 `run_end`；只取元数据，**绝不读对话正文**（有单测验证 `last-assistant-message` 不进 Event）。
- **sink/file.rs（FileSink）**：写 `events/.tmp/{ts}_{uuid}.json` 后 `rename` 进 `events/`，原子、避免半截。
- **main.rs（红线）**：`--source claude|codex` → adapter → sink；**任何异常都 `exit(0)`**；stdin 读取设 2s 超时；不发网络。
- **平台相关仅一处**：`host_name()` 当前用 Windows env，Linux 需改 `gethostname`（见 §8）。

### 5.3 App 后端（`src-tauri`）

| 模块 | 职责 | 平台性 |
|------|------|--------|
| `source/fs_watch.rs` | 启动**排空**现存文件→监听新文件→读取+删除（消费）→坏文件跳过；非递归监听避开 `.tmp` | ✅ 跨平台（notify） |
| `state.rs`（**StateMachine**） | 事件序列 → 状态/计时/`Effect::Completed`；纯逻辑、时间靠参数注入 | ✅ 纯跨平台 |
| `config.rs` | `config.json` 读写、坏文件回退默认 | ✅ 跨平台 |
| `installer/`（待实现，任务10-11） | 向 Claude `settings.json`/Codex `config.toml` **追加**接入条目：幂等、写前备份、可一键卸载、Codex notify 冲突检测 | ✅ 逻辑跨平台；**注意 reporter 路径不带 `.exe`** |
| `notify.rs`（待实现，任务12） | 完成时桌面通知+提示音，受配置开关 | ⚠️ 后端不同（见 §7.4） |
| `tray.rs` + `main.rs`（待实现，任务13） | 托盘/置顶小窗/关窗退托盘；装配 Source→channel→StateMachine→emit | ⚠️ 托盘 Linux 需 AppIndicator |

### 5.4 前端（Vanilla TS）— 跨平台
- 卡片渲染（CLI 标识、cwd、状态圆点、计时）、**本地 1s 秒表**、按 `host` 分组、`timing_reliable=false` 时标注"估算/不可用"。
- 通过 `@tauri-apps/api` 监听 `sessions:update` 事件、调用 install/uninstall/status/config 命令。

---

## 6. 状态机规格（核心逻辑）

```
RunStart  → status=Running, run_started_at=ts, run_ended_at=None, timing_reliable=true
RunEnd    → 若有"本轮"的 run_started_at: duration=ts-start, reliable=true（并消费该 start）
            否则:                          duration=None,      reliable=false   ← Codex 退化
            置 status=Done, run_ended_at=ts, 发 Effect::Completed
SessionEnd→ 移除会话
tick(now) → Done 且 now-ended ≥ idle_threshold ⇒ Idle
```

要点：
- **每个 RunEnd 都发一次 Completed**（多会话先后完成各自通知，不吞）。
- **乱序/缺失容错**：`RunEnd` 无前置 `RunStart` 仍标 `Done` 但 `timing_reliable=false`（Codex 只有回合完成时即此情形）。
- 计时**消费**起点：算完即清 `run_started_at`，使下一次无 start 的 `RunEnd` 正确退化。
- 快照排序：`Running` → `Done` → `Idle`，组内按最近活动倒序。

---

## 7. Linux 端实现指南

> 好消息：Linux 的**工具链与构建远比本机 Windows 简单**（无 mingw/dlltool/非 ASCII TMP 问题）。纯逻辑部分（csm-core / reporter / 状态机 / config / fs_watch）**几乎零改动**即可在 Linux 编译通过；主要工作量在 GUI 的系统依赖与托盘。

### 7.1 优先级建议（结合你的远端场景）
你常在**远端服务器（Linux）跑 Codex**。因此 Linux 上**最有价值的是先把 `session-reporter` 跑起来**——它运行在 CLI 所在的机器上、产出事件；Phase 2 配合网络 sink 即可把远端事件推回你的观看端。**Linux GUI 是次要的**（仅当你想在 Linux 桌面上"看"时才需要）。

推荐顺序：**① reporter（Linux）→ ② 验证文件总线 → ③（可选）Linux GUI → ④ Phase 2 远端推送**。

### 7.2 工具链
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
# 原生 cc/ld 即可，无需 mingw；cargo test 直接绿
```

### 7.3 路径与 XDG 约定
- **最简方案（推荐，跨平台一致）**：沿用 `~/.cli-session-monitor/{events,config.json}`。与 Claude/Codex 自己用 `~/.claude`、`~/.codex` 的 dotfolder 风格一致，reporter 与 App 无需平台分支。
- **XDG 方案（更"Linux 原生"）**：数据放 `$XDG_DATA_HOME`（默认 `~/.local/share/cli-session-monitor`），配置放 `$XDG_CONFIG_HOME`（默认 `~/.config/cli-session-monitor`）。用 `directories` crate 的 `ProjectDirs` 实现。
- **决策**：MVP 建议先用最简方案（最少分支）；要 XDG 合规时在 §3.2 的 `paths` 模块里按 `cfg!(target_os="linux")` 切换，**只动一处**。

### 7.4 通知后端
- `tauri-plugin-notification` 在 Linux 走 `org.freedesktop.Notifications`（D-Bus），需要桌面通知守护（GNOME/KDE/XFCE 等自带；纯无头服务器无 GUI 守护时通知不可用——但服务器一般只跑 reporter，不跑 App）。
- 提示音：Linux 上可用 `libcanberra`/直接播放 `assets/notify.wav`（如用 `rodio` 跨平台播放，避免依赖系统音频细节）。

### 7.5 系统托盘（最需注意）
- Tauri 托盘在 Linux 依赖 **AppIndicator/StatusNotifierItem**：
  ```bash
  sudo apt install libayatana-appindicator3-dev   # 或 libappindicator3-dev
  ```
- **GNOME** 默认不显示托盘，需用户装 “AppIndicator and KStatusNotifierItem Support” 扩展。KDE/XFCE/Cinnamon 一般开箱可用。
- 设计上：Linux GUI 若托盘不可用，应**降级**为普通常驻窗口（不强依赖托盘）。

### 7.6 Tauri GUI 的 Linux 系统依赖（Debian/Ubuntu 示例）
```bash
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  build-essential curl wget file libssl-dev \
  libgtk-3-dev librsvg2-dev \
  libayatana-appindicator3-dev
```
（Fedora/Arch 有对应包名：`webkit2gtk4.1-devel` / `webkit2gtk` 等。）

### 7.7 打包
- Tauri 可产 **AppImage**、**.deb**、**.rpm**。CI 里用 `tauri build` 产出对应格式。
- 纯 reporter 不需 GUI 依赖，`cargo build --release -p csm-reporter` 即可，产物 `target/release/session-reporter`，体积小、可单独分发到服务器。

### 7.8 CI matrix（任务17 扩展）
```yaml
strategy:
  matrix:
    os: [windows-latest, ubuntu-latest]   # 可加 macos-latest
# Linux job: apt 安装上面的 GUI 依赖 → cargo test → tauri build
# 纯逻辑测试两平台都跑；真实 CLI 测试按约定推迟
```

---

## 8. 当前实现状态与 Linux 需改动清单

**已完成并在 Windows(GNU) 验证（`cargo test` 43 passed / 1 ignored / 0 failed）**：任务 1–9 = workspace + csm-core + 完整 reporter + 后端引擎(config/状态机/fs_watch)。

**在 Linux 跑起来需要的改动（很少）**：

1. **`host_name()` 改跨平台**（唯一的真平台 bug）。当前 `crates/csm-reporter/src/adapter/mod.rs`：
   ```rust
   // 现状（Linux 下常返回 "unknown-host"）：
   std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME"))...
   // 建议：加依赖 gethostname = "0.5"，统一两平台
   pub fn host_name() -> String {
       gethostname::gethostname().to_string_lossy().into_owned()
   }
   ```
   （并把它上移到 §3.2 的共享 `paths` 模块，reporter 与 App 共用。）
2. **路径收敛**（可选但推荐）：reporter `main.rs`、`config.rs`、`fs_watch.rs` 三处的 `home_dir().join(".cli-session-monitor")` 收敛到共享函数；要 XDG 时只改这里。
3. **安装器（任务10-11，尚未实现）**：写 hook/notify 命令时用 reporter 的**实际绝对路径**（Linux 无 `.exe`）；从一开始就按跨平台写，无需后补。
4. **GUI（任务12-16，尚未实现）**：按 §7.4/§7.5/§7.6 处理通知后端、托盘、系统依赖。
5. **构建**：Linux 无需本机 Windows 那套 mingw/TMP workaround。

> 结论：**reporter + 后端引擎在 Linux 上基本是"改一个 host_name + 装好依赖"即可编译运行**；真正的平台工作量集中在 GUI 托盘与打包。

---

## 9. Codex 的待验证点（两平台共同）
Codex `notify` 能触发的事件可能少于 Claude（可能只有"回合完成/需审批"，**不一定有干净的"开始"信号**），其 payload 是否含稳定 `session_id` 亦需实测。实现第一步（Codex 装好后）跑**能力探针**：实测它实际发出的事件类型与字段，据此确认 Codex 计时是**实时秒表**还是退化为**只显示上次耗时/完成时间**（状态机已支持后者：`timing_reliable=false`）。

---

## 10. 错误处理 / 红线（两平台一致）

| 场景 | 处理 | 用户影响 |
|------|------|----------|
| reporter 内部任何异常 | 兜底，仍 `exit(0)` | CLI 完全不受影响，该事件丢失（下次纠正）|
| 事件文件损坏/schema 不识别 | 跳过+记日志，并删除junk | 不显示该条，App 不崩 |
| `events/` 不存在 | App/Reporter 各自确保创建 | — |
| fs 事件洪峰 | 去抖/合并，消费即删 | UI 平滑，不飙 CPU |
| Claude/Codex 配置解析或写入失败 | 中止、还原备份、报错 | 不留损坏配置 |
| Codex `notify` 已被他人占用 | 不覆盖，回报用户 | 知情，不被静默改配置 |
| App 未运行时 CLI 触发事件 | reporter 照写，文件累积，App 启动排空补读 | 重启 App 自动恢复 |
| 缺失/乱序事件 | 状态机容错（`timing_reliable=false`）| 卡片标注估算 |

**最高红线**：reporter 绝不阻塞/破坏 CLI（短超时 + 永远 exit 0）；reporter 只取元数据、**绝不存对话正文**。

---

## 11. 测试策略
- **纯逻辑/集成单测（保留、两平台都跑）**：reporter 归一化（样例 JSON）、状态机（事件序列）、config（临时目录）、安装器（临时配置：幂等/备份/卸载还原/Codex 冲突）、fs_watch 排空（临时目录投文件）。**不需任何 CLI、不碰真实配置**。
- **真实 CLI 测试（推迟）**：真实 Claude/Codex 端到端冒烟、Codex 能力探针——按用户约定推迟到环境就绪。
- live fs watcher 测试标 `#[ignore]`，保 CI 确定性，本地 `--ignored` 手动跑。

---

## 12. 安全与隐私
- MVP **纯本地**，不发网络、不采集/不传对话正文，仅在用户专属目录读写。
- 配置注入：**只追加、写前备份、幂等、可一键卸载还原**——这是开源信任的基石（README 专列透明章节）。
- Phase 2 远端见 §13 的权衡与缓解。

---

## 13. Phase 2：远端 / 跨设备（与 Linux 强相关）

用户场景：常在**远端服务器**（多为 Linux，且可能**无 SSH**、可能在**另一台设备/手机**上看）跑 Codex。三者叠加 ⇒ 事件必须经一个网络可达的**中继/发布-订阅**端点，**无法保持"纯本地"**（这是该场景的固有代价）。

- **传输抽象已就位**：reporter 加 `NetworkSink`（推送到中继，带鉴权 token、最小元数据）；App 加 `NetworkSource`（订阅中继）。schema 已含 `host`、`SessionKey` 已含 `host`，UI 可按来源分组 —— **无需改动状态机**。
- **中继候选**：自托管轻量中继 / **复用 ntfy 式 pub-sub**（与用户既有习惯一致）/（同设备同机子情况）反向 SSH 隧道。
- **缓解**：鉴权 token、最小元数据（绝不发正文、可隐去路径）、**自托管中继**选项使数据留在自有基础设施。
- **Linux 角色**：远端 Linux 上跑的就是**跨平台 reporter**——这也是为什么优先把 reporter 移植到 Linux：它天然是"远端事件源"。

---

## 14. 目录结构

```
cli-session-monitor/
├─ Cargo.toml                # [workspace]
├─ crates/
│  ├─ csm-core/              # 事件 schema + SessionKey（建议再放 paths/host_name 平台层）
│  └─ csm-reporter/          # session-reporter：adapter/{claude,codex} + sink/{file,(network)}
├─ src-tauri/                # App 后端：source/{fs_watch,(network)} + state + config + installer + notify + tray
├─ src/                      # 前端：卡片 + 秒表 + 设置（Vanilla TS）
├─ docs/DESIGN.md            # 本报告
├─ .github/workflows/ci.yml  # CI（Windows + Linux matrix）
└─ README / LICENSE(MIT) / CHANGELOG
```

---

## 15. 路线图

- **MVP（进行中，9/17 已验证）**：Win 本地——reporter×2、文件总线、fs 监听、状态机、卡片+秒表、托盘、完成通知、一键安装/卸载。
- **跨平台（本报告新增目标）**：Linux 端 reporter（最小改动）→ Linux GUI（托盘/依赖/打包）。
- **Phase 2**：远端/跨设备（中继 / ntfy 式 pub-sub）+ 鉴权 + 自托管。
- **Phase 2+（未定）**：点击聚焦终端窗口、历史统计、macOS、开机自启。
