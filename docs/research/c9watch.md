# c9watch 调研报告

> 调研对象：[minchenlee/c9watch](https://github.com/minchenlee/c9watch) · 抓取时间 2026-08-01 · 所有结论基于 `main` 分支源码 raw 内容

## 1. 概述

| 维度 | 信息 |
|------|------|
| 仓库 | https://github.com/minchenlee/c9watch |
| 语义 | **c**laude cod**e** watch（类比 k8s ← Kubernetes） |
| 定位 | macOS menubar 常驻 + CLI 二合一，监控本机所有 Claude Code 会话；同时给人用（GUI）也给 agent 用（JSON CLI） |
| Stars / Forks | 114 / 32（调研时） |
| 创建时间 | 2026-02-06，最近更新 2026-07-31（活跃维护） |
| License | MIT |
| 版本 | v0.8.1（tauri.conf.json `version`） |
| 技术栈 | Tauri 2 + SvelteKit + Svelte 5 + Rust；进程发现用 [sysinfo](https://crates.io/crates/sysinfo)；NSPanel 用 [tauri-nspanel](https://crates.io/crates/tauri-nspanel)；通知用 `tauri-plugin-notification`；移动端通过 WebSocket 扩展 |
| macOS 最低版本 | 10.15 |
| 设计系统 | Vercel Noir（纯黑 + Geist 字体） |

**与 cc-view 的契合度极高**：同样的技术栈（Tauri 2 + 前端 + Rust）、同样的产品形态（menubar 常驻、一键 focus、不漏"等我"）、同样的零侵入追求（不装 hook、靠 OS 级进程扫描自动发现）。下面的结论几乎可以逐条对照搬用。

---

## 2. 数据源与进程发现

### 2.1 数据源全景

c9watch **没有用** `~/.claude/daemon/roster.json`（这是 cc-view 当前的设计数据源之一）。它读取的是一组更底层、更权威的文件：

| 文件 / 来源 | 用途 | 备注 |
|-------------|------|------|
| `~/.claude/sessions/<pid>.json` | **PID → session_id 的权威映射**（Claude Code 自己写） | 拿 `sessionId` 字段；`/clear` 后会更新，所以总是对应当前会话 |
| `~/.claude/sessions/<pid>.json` 的 `entrypoint` 字段 | 区分 `cli` / `sdk-ts` / `sdk-py` | CLI 后端只保留 `cli`，过滤掉 Zed 等 IDE 的 SDK 会话 |
| `~/.claude/projects/<encoded-cwd>/<session_id>.jsonl` | 会话内容，**状态的唯一来源** | 尾部 20 条用于判状态；`agent-*.jsonl` 被跳过 |
| `~/.claude/projects/<encoded-cwd>/sessions-index.json` | session 元数据索引 | 含 `projectPath` / `firstPrompt` / `summary` / `messageCount` / `gitBranch` / `modified` |
| `~/.claude/settings.json` 的 `permissions.allow` | 权限白名单 | PermissionChecker 用它判"等权限 vs 自动放行" |
| `claude agents --json`（CLI 子命令，CC ≥ 2.1.145） | 进程列表 + busy/idle 状态 + 启动时间 | CLI 后端的主数据源 |
| `~/.claude/c9watch/workers/*/meta.json` | c9watch 自己的 PM 子会话 overlay | 拿 `spawnedBy` 标 worker_of；目录 mtime 变化时才重读 |
| `~/.claude/tasks/<session_id>/*.json` | TodoWrite 任务 | CLI 子命令读 |
| `~/.claude/history.jsonl` | 历史索引 | 历史页用 |
| `~/.claude/projects/*/snapshots/` (Codex) | OpenAI Codex CLI 的会话快照 | c9watch 额外支持 Codex（cc-view 用不到） |

> **关键差异提示**：cc-view 设计文档里提到的 `~/.claude/daemon/roster.json` 在 c9watch 中**未出现**。c9watch 走的是 sysinfo 进程扫描 + `sessions/<pid>.json` + `claude agents --json` 三路数据，没有依赖任何 daemon 聚合文件。如果 roster.json 不是 Claude Code 官方稳定接口，c9watch 这套组合可能更稳。

### 2.2 双后端架构（CLI 优先 + Legacy 兜底）

c9watch 有两个 `SessionSource` 实现，启动时按 Claude Code 版本自动选择（`src-tauri/src/session/mod.rs::create_session_source`）：

```
BackendMode::Auto (默认)
  ├─ probe_claude_supports_agents_json()
  │   ├─ `claude --version` 拿 semver → ≥ (2,1,145)?
  │   └─ `claude agents --json` 能跑且输出是合法 JSON 数组?
  ├─ 都通过 → CliSessionSource (优先)
  └─ 任一失败 → LegacySessionSource (兜底)
ForceCli / ForceLegacy 由 C9WATCH_DETECTOR_BACKEND 环境变量强制
```

运行时还有**自动降级**（`state.rs`）：CLI 后端连续 5 次失败 → 自动切回 Legacy；每次主窗口重新聚焦（`Focused(true)`）会 `recheck_and_maybe_swap` 重新探测，支持用户中途升级 Claude Code。

### 2.3 Legacy 后端：sysinfo 进程发现 + 双路 JSONL 匹配

> 源文件：[src-tauri/src/session/detector.rs](https://github.com/minchenlee/c9watch/blob/main/src-tauri/src/session/detector.rs)

**步骤 1 — 找 claude 进程**（`find_claude_processes`）：

```rust
// sysinfo 只刷新需要的字段（exe / cmd / cwd），用 OnlyIfNotSet 避免重复拉取
self.system.refresh_processes_specifics(
    ProcessesToUpdate::All, true,
    ProcessRefreshKind::new()
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::OnlyIfNotSet)
        .with_cwd(UpdateKind::OnlyIfNotSet),
);

for (pid, process) in self.system.processes() {
    let name = process.name().to_string_lossy();
    if name.contains("c9watch") { continue; }  // 排除自己

    // 关键：macOS 上 npm 安装的 Claude Code，process.name() 返回 "node"
    // 必须再查 cmd() 的 args 数组里有没有 "claude"
    let name_match = name.contains("claude");
    let cmd_match = !name_match && process.cmd().iter().any(|arg| {
        let a = arg.to_string_lossy();
        a.contains("claude") && !a.contains("c9watch")
    });

    if name_match || cmd_match {
        processes.push(ClaudeProcess {
            pid: pid.as_u32(),
            cwd: process.cwd().map(|p| p.to_path_buf()),
            start_time: process.start_time(),  // 秒级 Unix 时间戳
        });
    }
}
```

> **踩坑提示（来自 `DetectionDiagnostics`）**：如果 `claude_processes_found > 0` 但 `processes_with_cwd == 0`，几乎一定是用户没给 c9watch **Full Disk Access**（系统设置 → 隐私与安全 → 完全磁盘访问权限）。诊断事件会通过 `diagnostic-update` 推给前端高亮提示。cc-view 必然会遇到同样的问题。

**步骤 2 — 把 PID 匹配到 session JSONL 文件**（`find_active_sessions`，**两层 fallback**）：

```
对每个 process（按 start_time 倒序，新的优先）:

  [Primary] 读 ~/.claude/sessions/<pid>.json 拿 session_id
    └─ 命中且 session_id 未被占用 → 在所有 project_dir 里找 <session_id>.jsonl
       → 用 sessions-index.json 的 projectPath 字段拿真实 cwd

  [Fallback] 当 pid.json 不存在（老版本 Claude Code）:
    ├─ 把 proc.cwd 用 encode_path_for_matching 编码
    │   规则：每个非 [a-zA-Z0-9] 字符 → '-'
    │   例: /Users/Name/My_Project → -Users-Name-My-Project
    ├─ 用编码后的字符串和 project_dir 的目录名直接比对
    ├─ 或者用 sessions-index.json 里的 project_path 做
    │   "proc.cwd == project_path || proc.cwd.starts_with(project_path)"
    └─ 关键时间戳约束：
        session_mtime + 5s >= proc.start_time
        （session 修改时间必须 ≥ 进程启动时间 - 5s 缓冲）
        → 防止新启动的 Claude 进程（还没写 JSONL）误匹配到同目录的老 session
```

匹配过程用 `used_session_ids: HashSet` 保证一个 session 只能被一个进程认领。所有失败路径都通过 `debug_log::log_warn` 落盘，方便用户用 `Cmd+Shift+D` 调试面板排查。

### 2.4 CLI 后端：直接调 `claude agents --json`

> 源文件：[src-tauri/src/session/detector_cli.rs](https://github.com/minchenlee/c9watch/blob/main/src-tauri/src/session/detector_cli.rs)

```rust
// spawn `claude agents --json`，2 秒超时
// 必须用后台线程 drain stdout，否则子进程写满 OS 管道缓冲（~64KB）会死锁
let mut child = Command::new(&self.claude_bin)
    .args(["agents", "--json"])
    .stdout(Stdio::piped()).stderr(Stdio::null())
    .spawn()?;
let reader = std::thread::spawn(move || { /* read_to_end */ });
match child.wait_timeout(Duration::from_secs(2))? {
    Some(status) if status.success() => { /* parse JSON */ }
    None => { child.kill(); child.wait(); /* reap 防 zombie */ }
}

// 解析得到 CliAgent { pid, cwd, kind, startedAt, sessionId, name, status }
// status: Option<String> 取值 "busy" / "idle" → 映射为 CliActivity::Busy / Idle

// 过滤：读 ~/.claude/sessions/<pid>.json 的 entrypoint 字段
// 只保留 entrypoint == "cli"，排除 sdk-ts（Zed 等 IDE）/ sdk-py
// 文件缺失或字段缺失 → 保留（兼容老版本）
```

CLI 后端拿到 `startedAt`（毫秒时间戳）和 `status`（busy/idle），这是 Legacy 后端**拿不到**的信息。`merge_cli_activity` 决定如何融合：

```rust
// 永远不覆盖 NeedsAttention / Connecting（这两个由 JSONL + PermissionChecker 决定）
// 只能在 Working ↔ WaitingForInput 之间 refine
match (heuristic, cli) {
    (NeedsAttention, _) => NeedsAttention,
    (Connecting, _)    => Connecting,
    (WaitingForInput, Some(Busy))                 => Working,    // CLI 说忙就忙
    (Working, Some(Idle)) if !has_pending_tool     => WaitingForInput, // CLI 说闲且无待审工具→真闲
    (heuristic, _) => heuristic,
}
```

**cc-view 的启示**：如果 cc-view 能要求用户使用 CC ≥ 2.1.145，强烈建议主走 CLI 后端——`status:busy/idle` 直接来自 Claude Code 内部，比启发式判定更准；JSONL 状态机只负责识别 NeedsAttention（权限/提问）。

---

## 3. 状态判定逻辑（最关键）

### 3.1 四个状态（不是五个）

> 源文件：[src-tauri/src/session/status.rs](https://github.com/minchenlee/c9watch/blob/main/src-tauri/src/session/status.rs)

```rust
pub enum SessionStatus {
    Working,          // 正在执行工具 / 生成回复 / 思考
    NeedsAttention,   // 等权限 / 等用户回答提问
    WaitingForInput,  // 空闲，等待下一条 prompt（相当于 cc-view 的 "Done/Idle"）
    Connecting,       // 启动中，或最近 N 条全是 progress 无实质消息
}
```

**没有 "Done" 状态**——cc-view 设计里的 "done" 在 c9watch 里就是 `WaitingForInput`。语义上 Claude Code 会话永不真正结束，只是把球交回给用户。**建议 cc-view 也合并 Done 与 Idle**，避免状态机臃肿。

### 3.2 输入：JSONL 末尾 20 条

```rust
// enrichment.rs
let entries = parse_last_n_entries(&session_file_path, 20)?;
let status = determine_status(&entries);
```

`parse_last_n_lines`（parser.rs）的实现细节值得抄：

- 文件 < 10KB：直接全读，取末尾 N 行
- 文件 ≥ 10KB：`SeekFrom::End(-(n * 1024 * 2))` 跳到末尾 n*2KB 处再正向读，避免大文件全量加载
- 所有空行过滤掉
- `parse_jsonl_entries` 用 `serde(tag = "type")` 把每行解析成 `SessionEntry` enum，无法识别的 `type` 一律落到 `Unknown`（**容错关键**，因为 Claude Code 会写 `progress` / `file-history-snapshot` 等非主流类型）

### 3.3 状态机判定规则（贴关键代码）

`determine_status` 函数完整流程：

```rust
pub fn determine_status(entries: &[SessionEntry]) -> SessionStatus {
    if entries.is_empty() { return Connecting; }

    // 找最后一个 User 或 Assistant entry，跳过 progress / file-history-snapshot / summary
    let last_meaningful = entries.iter().rev()
        .find(|e| matches!(e, SessionEntry::User{..} | SessionEntry::Assistant{..}));

    // 关键：检查 last_meaningful 之后是否有 trailing Unknown（= progress 条目）
    // progress 条目表示工具正在执行（如 bash_progress）
    let last_meaningful_idx = entries.iter().rposition(|e| /* User|Assistant */);
    let has_trailing_progress = entries[last_meaningful_idx + 1..]
        .iter().any(|e| matches!(e, SessionEntry::Unknown));

    match last_entry {
        // ─── User entry ───────────────────────────────────
        SessionEntry::User { base, message } => {
            if message.is_tool_result {
                // 工具结果（user 消息但 content 是 tool_result 数组）
                // Claude 应该正在生成下一条回复
                if is_entry_recent(&base.timestamp, 30) { Working }
                else { WaitingForInput }   // 30s 没动静 = 进程可能死了
            } else if is_entry_recent(&base.timestamp, 30) {
                Working   // 刚发完 prompt，Claude 在响应
            } else {
                WaitingForInput   // 老的 prompt 没响应，会话闲置
            }
        }

        // ─── Assistant entry ──────────────────────────────
        SessionEntry::Assistant { base, message } => {
            // 规则 A: AskUserQuestion 工具挂起 → 立即 NeedsAttention（无延迟）
            if has_pending_ask_user_question(&message.content) {
                return NeedsAttention;
            }
            // 规则 B: 文本末尾以 '?' 或 '?)' 结尾 + stop_reason=end_turn
            //         + 时间超过 20s → NeedsAttention（20s 防抖避免流式生成时闪烁）
            if is_assistant_asking_question(message) && !is_entry_recent(&base.timestamp, 20) {
                return NeedsAttention;
            }

            match analyze_assistant_message(message) {
                Working => {
                    if has_pending_tool_uses(&message.content) {
                        // 工具调用未完成 → 看 trailing_progress 或 20s 内有活动
                        if has_trailing_progress || is_entry_recent(&base.timestamp, 20) {
                            Working
                        } else {
                            Working   // 注意：这里源码两个分支都是 Working，注释说"stale 但仍标 Working"
                        }
                    } else {
                        // 纯文本/thinking，没工具调用
                        // stop_reason 在 JSONL 里永远是 None，只能靠时间戳
                        if is_entry_recent(&base.timestamp, 20) { Working }
                        else { WaitingForInput }
                    }
                }
                NeedsAttention => NeedsAttention,  // 权限等待，立即返回
                _ => raw_status,
            }
        }
        _ => WaitingForInput,
    }
}
```

`analyze_assistant_message` 的子规则：

| 内容 | stop_reason | 工具状态 | 返回 |
|------|-------------|----------|------|
| 含 ToolUse | - | 部分未完成 + 全部 auto-approved | **Working** |
| 含 ToolUse | - | 部分未完成 + 至少一个需权限 | **NeedsAttention** |
| 含 ToolUse | - | 全部已完成 | 看 stop_reason → `end_turn` = WaitingForInput |
| 纯文本/thinking | `end_turn` / `max_tokens` / `stop_sequence` | - | WaitingForInput |
| 纯文本/thinking | `None` | - | Working（仍在生成） |

### 3.4 "等权限" 怎么判：PermissionChecker（核心创新点）

> 源文件：[src-tauri/src/session/permissions.rs](https://github.com/minchenlee/c9watch/blob/main/src-tauri/src/session/permissions.rs)

c9watch 最聪明的设计是：**不靠等用户实际点击"允许"才识别 NeedsAttention，而是在 JSONL 出现 ToolUse 的瞬间，根据 Claude Code 自己的 `~/.claude/settings.json` 权限白名单预测这次调用会不会触发权限弹窗**。

```rust
pub fn is_auto_approved(&self, tool_name: &str, tool_input: &serde_json::Value) -> bool {
    // 永远 auto-approve（只读工具）
    match tool_name {
        "Read" | "Glob" | "Grep" | "WebFetch" | "WebSearch"
        | "Task" | "TaskList" | "TaskGet" | "TaskCreate" | "TaskUpdate"
        | "AskUserQuestion" => return true,
        _ => {}
    }

    if tool_name == "Bash" {
        let command = tool_input.get("command").and_then(|c| c.as_str()).unwrap_or("");
        return self.is_bash_allowed(command);   // 匹配 Bash(prefix:*) / Bash(cmd)
    }

    if matches!(tool_name, "Write" | "Edit" | "NotebookEdit") {
        return self.is_tool_allowed(tool_name); // 显式白名单
    }

    if tool_name.starts_with("mcp__") {
        return self.is_mcp_allowed(tool_name);
    }

    false  // 默认需要权限（保守）
}
```

`~/.claude/settings.json` 的 `permissions.allow` 数组按以下格式解析：

| Pattern 例子 | 解析为 |
|--------------|--------|
| `Bash(git add:*)` | Bash { prefix: "git add", wildcard: true } |
| `Bash(npm ci)` | Bash { prefix: "npm ci", wildcard: false }（精确匹配） |
| `Read` | Tool { name: "Read" }（整类工具放行） |
| `mcp__atlassian__getJiraIssue` | Mcp { name: ... } |
| `Skill(name)` | Skill { name: ... } |

Bash 的匹配规则：wildcard=true 用 `starts_with(prefix)`；wildcard=false 必须 trim 后相等。**坑**：精确模式下 `npm ci --legacy-peer-deps` 不算匹配 `Bash(npm ci)`。

> PermissionChecker 用 `OnceLock` 全局单例，首次访问时从 `~/.claude/settings.json` 加载一次，之后整个进程生命周期不刷新。**局限**：用户在 Claude Code 运行中改 settings.json 不会立即反映——cc-view 如果做，建议加 mtime 监听或每次轮询时检查一遍。

`get_pending_tool_name` 还会返回具体卡在哪个工具，供 UI 直接展示。比如返回 `"Bash"` 表示卡在 Bash 权限，`"AskUserQuestion"` 表示 Claude 在问问题，`"Question"` 表示文本提问。

### 3.5 文件 mtime 兜底（防假阴性）

enrichment.rs 里有一个**非常重要的兜底**，弥补 JSONL 状态机的延迟：

```rust
let heuristic_status = if entries.is_empty() {
    SessionStatus::Connecting
} else {
    let raw_status = determine_status(&entries);
    // 关键：如果判出 WaitingForInput 但 JSONL 文件 8 秒内被修改过 → 升级为 Working
    // 这是为了处理"流式写入"场景：Claude 正在生成，但状态机因为 stop_reason=None + 时间戳旧
    // 误判为 Idle
    if raw_status == SessionStatus::WaitingForInput
        && is_file_recently_modified(&session_file_path, 8) {
        SessionStatus::Working
    } else {
        raw_status
    }
};
```

---

## 4. 架构与关键模块

### 4.1 目录结构（src-tauri/src/）

```
src-tauri/src/
├── lib.rs              ★ App 入口、Tauri commands、NSPanel setup、tray、exit prevent
├── main.rs             # fn main() { ccview_lib::run() }
├── actions.rs          # open_session (focus 终端) / stop_session (kill) / iTerm2 标题
├── auth.rs             # WebSocket token 生成、本机 IP 发现（移动端配对用）
├── debug_log.rs        # 内存环缓冲日志，Cmd+Shift+D 可查看
├── polling.rs          ★ 后台轮询循环、状态转换检测、通知、emit
├── web_server.rs       # WebSocket 服务器（给移动端/网页端用，cc-view 暂不需要）
└── session/
    ├── mod.rs          ★ 模块导出 + create_session_source 工厂 + claude --version 探测
    ├── source.rs       # SessionSource trait、DetectedSession、各种 enum
    ├── state.rs        # DetectorState (Arc<Mutex>) + 连续失败降级 + 重新聚焦时重探
    ├── detector.rs     ★ LegacySessionSource (sysinfo + pid.json + 路径编码 + 时间戳)
    ├── detector_cli.rs ★ CliSessionSource (claude agents --json + entrypoint 过滤)
    ├── parser.rs       ★ JSONL 解析、sessions-index.json、read_last_n_lines、is_system_content
    ├── status.rs       ★★ SessionStatus 状态机（最核心）
    ├── permissions.rs  ★ PermissionChecker（auto-approve 判定）
    ├── enrichment.rs   ★ Session 完整结构、enrich_detected_sessions、merge_cli_activity
    ├── conversation.rs # 单会话完整对话加载
    ├── history.rs      # history.jsonl 索引 + 跨 project 深度搜索
    ├── cost.rs         # 按 model 算 token cost，mtime 缓存
    ├── memory.rs       # 读 ~/.claude/memory files
    ├── custom_names.rs # 用户自定义会话名（持久化）
    ├── sanitize.rs     # 剥离系统 XML 标签（<bash-stdout> 等）
    ├── subagents.rs    # Task/Agent 子代理转录
    ├── codex.rs        # Codex CLI 支持
    └── codex_archive.rs
└── cli/                # PM（Process Manager）编排子会话（spawn/send/workers/inbox）
    ├── mod.rs
    ├── pm.rs / pm_daemon.rs / pm_worker.rs / pm_caller.rs / pm_fs.rs / pm_inbox.rs / pm_rpc.rs
    ├── bg_backend.rs / bg_protocol.rs
    ├── adoption.rs
    └── worker_backend.rs
```

### 4.2 关键依赖（Cargo.toml 推断）

- `sysinfo` — 进程扫描
- `serde` / `serde_json` — 反序列化
- `chrono` — 时间戳比较
- `dirs` — `home_dir()`
- `tokio` (broadcast channel) — 后台事件广播给 WebSocket
- `tauri-nspanel` — NSPanel popover
- `tauri-plugin-notification` — 原生通知
- `tauri-plugin-updater` / `tauri-plugin-process` — 自动更新
- `wait-timeout` — `claude agents --json` 子进程超时
- `libc` — `kill(pid, 0)` 探活
- `base64` — 图片解码
- `qr2term` — 终端打印二维码（移动端配对）

---

## 5. Tauri 事件流 + NSPanel Popover

### 5.1 轮询循环（polling.rs）

> 源文件：[src-tauri/src/polling.rs](https://github.com/minchenlee/c9watch/blob/main/src-tauri/src/polling.rs)

**核心参数**：
- 轮询间隔 **3.5s**（不是 README 说的 2s，源码 `Duration::from_millis(3500)` 是真相）
- 通知冷却 **30s**（同一 session 的同一类通知 30s 内不重复）
- 状态 hash 去重（`DefaultHasher`）

**单次循环逻辑**：

```
1. 短时锁 detector，执行 detect()，立即释放锁
   └─ 关键：detect 完就放锁，slow enrichment 在锁外做
      不阻塞 Tauri commands（get_sessions 等）

2. enrich_detected_sessions(detected) → Vec<Session>

3. overlay workers map（从 ~/.claude/c9watch/workers/ 读 spawnedBy）
   └─ 用 WorkersCache 缓存，目录 mtime 变了才重读

4. 状态转换检测：
   for each session:
     prev_status = previous_status.get(session.id)
     # 只关心一种转换：Working → NeedsAttention | WaitingForInput
     # （即"刚需要用户"或"刚干完"）
     if prev == Working && now in [NeedsAttention, WaitingForInput]:
       if !on_cooldown (30s):
         fire_notification(...)
         last_notification_time[sid] = Instant::now()
     previous_status[sid] = now

5. 序列化 sessions → JSON → hash
   └─ 仅当 hash 变化时才 emit("sessions-updated", sessions)
      避免空闲 dashboard 每 3.5s rerender 一次

6. 诊断信息 emit("diagnostic-update", ...) 也只在变化时发
```

**通知文案**（`fire_notification`）：

| 状态 | pending_tool_name | 文案 |
|------|-------------------|------|
| NeedsAttention | `"Question"` / `"AskUserQuestion"` | `❓ {session_name}: Waiting for your response` |
| NeedsAttention | 其他（Bash/Write/...） | `🔐 {session_name}: Needs permission for {tool_name}` |
| WaitingForInput | - | `✅ {session_name}: Finished working` |

通知同时走三路：`tauri-plugin-notification`（原生弹窗）+ `emit("notification-fired", metadata)`（前端处理 click-to-focus）+ WebSocket broadcast（移动端）。`notification_id` 用 session_id 的 hash 模 i32，保证同一 session 通知稳定可定位。

### 5.2 Tauri 事件清单

| 事件名 | 方向 | payload | 触发 |
|--------|------|---------|------|
| `sessions-updated` | Rust → FE | `Vec<Session>` | 每次 poll 且 hash 变化 |
| `diagnostic-update` | Rust → FE | `DetectionDiagnostics` | FDA 状态变化等 |
| `notification-fired` | Rust → FE | `NotificationMetadata { sessionId, pid, projectPath, title }` | 状态转换触发通知时 |

**前端 invoke 的 Tauri commands**（`lib.rs::invoke_handler`）：

```
get_sessions, get_conversation, get_session_history, deep_search_sessions,
get_cost_data, get_memory_files, get_subagents, get_subagent_transcript,
get_session_tasks, save_temp_image, reveal_in_file_manager,
stop_session, open_session, rename_session,
get_terminal_title, show_main_window, get_server_info, get_debug_logs, greet
```

注意 `get_sessions` 在用户主动打开 dashboard 时即时调用一次（不等下一个 poll 周期），保证开窗瞬间有数据。

### 5.3 NSPanel Popover（macOS）

> 源文件：[src-tauri/src/lib.rs](https://github.com/minchenlee/c9watch/blob/main/src-tauri/src/lib.rs) + [src-tauri/tauri.conf.json](https://github.com/minchenlee/c9watch/blob/main/src-tauri/tauri.conf.json)

**tauri.conf.json 窗口声明**：

```json
{
  "label": "popover",
  "url": "/popover",
  "width": 320, "height": 400,
  "visible": false,            // 启动隐藏
  "resizable": false,
  "decorations": false,        // 无标题栏
  "transparent": true,         // 透明背景（圆角靠 Rust 设 corner_radius）
  "alwaysOnTop": true,
  "skipTaskbar": true,
  "shadow": false              // 系统 shadow 和 NSPanel corner_radius 二选一
}
```

**lib.rs setup 阶段把 NSWindow 升级为 NSPanel**（关键，普通 NSWindow 无法浮在 fullscreen app 之上）：

```rust
// 用 tauri-nspanel 的 to_panel() 转换
let panel = popover.to_panel::<PopoverPanel>()?;

// 1. Level = Status (25)，和 macOS 菜单栏同级
panel.set_level(PanelLevel::Status.value());

// 2. 关键：NonactivatingPanel style mask
//    使 panel 接管键盘输入但不抢活跃 app 焦点
//    → 全屏 IDE/编辑器不会因为开 popover 而失焦
panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());

// 3. CollectionBehavior：三选一组合
//    full_screen_auxiliary: 能浮在 fullscreen app 之上
//    can_join_all_spaces:   切 Space 不消失
//    stationary:            不参与 Space 切换动画
panel.set_collection_behavior(
    CollectionBehavior::new()
        .full_screen_auxiliary()
        .can_join_all_spaces()
        .stationary()
        .into(),
);

// 4. App 失活时不隐藏（否则切到别的 app popover 就没了）
panel.set_hides_on_deactivate(false);

// 5. 10pt 圆角（在 NSWindow 层做，比 CSS 圆角更干净）
panel.set_corner_radius(10.0);

// 6. 点击外部自动关闭：监听 window_did_resign_key 事件
let handler = PopoverEventHandler::new();
handler.window_did_resign_key(move |_| {
    if let Ok(p) = handle.get_webview_panel("popover") { p.hide(); }
});
panel.set_event_handler(Some(handler.as_ref()));
```

**Tray 点击定位**（`on_tray_icon_event`）：

```rust
// 只响应 Left + Up（防止 Down→Up 双触发）
TrayIconEvent::Click { button: Left, button_state: Up, rect, .. } => {
    if panel.is_visible() { panel.hide(); }
    else {
        // 把 popover 左上角对齐 tray icon 左下角
        let scale = popover.current_monitor()...scale_factor();
        let pos = rect.position.to_physical(scale);
        let size = rect.size.to_physical(scale);
        popover.set_position(PhysicalPosition::new(
            pos.x.round() as i32,
            (pos.y + size.height + 4.0).round() as i32,
        ));
        panel.show_and_make_key();
    }
}
```

**App 生命周期兜底**（menubar 常驻必需）：

```rust
// 主窗口的 CloseRequested → prevent_close + hide（不退出）
main_win.on_window_event(move |event| match event {
    WindowEvent::CloseRequested { api, .. } => {
        api.prevent_close();
        let _ = main_win_clone.hide();
    }
    WindowEvent::Focused(true) => {
        // 用户聚焦主窗口时重新探测 backend（支持 CC 中途升级）
        state.recheck_and_maybe_swap();
    }
    _ => {}
});

// App 级 ExitRequested → prevent_exit（窗口全关也不退出）
.run(|_app, event| {
    if let RunEvent::ExitRequested { api, .. } = event {
        api.prevent_exit();
    }
});
```

---

## 6. 对 cc-view 的可借鉴点（逐条对应）

### 6.1 状态判定（最重要）

| c9watch 做法 | cc-view 可直接复用 |
|--------------|---------------------|
| **4 状态而非 5**：合并 Done/Idle 为 `WaitingForInput` | 建议 cc-view 简化为同款 4 状态机；"done" 是 WaitingForInput 的子case（通过"刚从 Working 转来 + N 秒内"判定） |
| **PermissionChecker 预测式权限识别**：读 `~/.claude/settings.json` 的 `permissions.allow`，ToolUse 出现瞬间即可判 NeedsPermission，不用等用户实际点击 | **强烈建议 cc-view 抄这套**。直接解决"等权限怎么判"——而且比 cc-view 设计文档里的"等 statusUpdatedAt 字段变化"更早、更准。cc-view 的 `~/.claude/sessions/<pid>.json` 数据源里没有等价的 permission 字段，需要补这套白名单匹配逻辑 |
| **末尾 20 条 + is_entry_recent 时间戳兜底** | 直接复用；20 是个好数字（覆盖大多数"上一轮工具还在跑"场景，又不至于太慢） |
| **文件 mtime 8s 兜底**：判出 WaitingForInput 但 JSONL 8s 内被改过 → 升级为 Working | 直接复用；解决流式写入的假阴性 |
| **AskUserQuestion 立即触发 NeedsAttention**，文本问号有 20s 防抖 | 直接复用；防止 Claude 流式生成时一会儿 "?" 一会儿不是造成的闪烁 |
| **trailing Unknown (progress) 检测**：last_meaningful 之后还有 Unknown 条目 = 工具在跑 | 直接复用；这是识别 bash_progress 等长任务的关键 |
| **双后端（CLI + Legacy）自动切换** | cc-view 当前设计基于 `sessions/<pid>.json` + `roster.json`，更接近 Legacy 思路。建议未来加 CLI 后端作为增强（CC ≥ 2.1.145 时拿到 busy/idle 直说） |

### 6.2 零侵入实时性

| 维度 | c9watch 方案 | cc-view 借鉴 |
|------|--------------|--------------|
| 进程发现 | sysinfo 扫描 + `sessions/<pid>.json` + 路径编码三重匹配 | **完全可复用**；cc-view 设计的 `sessions/<pid>.json` 数据源正好是 c9watch 的主路径 |
| 实时性 | **纯轮询 3.5s**，未用任何 inotify/FSEvents 文件监听 | cc-view 可以放心用轮询；FSEvents 在 macOS 上对 `~/.claude/` 这种频繁小写入反而有性能和稳定性坑。如果想要更实时，建议对 `~/.claude/sessions/` 目录用 `FSEvents` 监听**新增文件**事件（触发立即重扫），但内容变化仍靠轮询 |
| 性能 | detect 锁内做完立即放锁，enrichment 在锁外慢慢跑；JSONL 用 seek-from-end 读；状态 hash 去重 | **三招全抄**：锁粒度优化、尾部读、hash 去重。这是 100+ 并发会话不卡 UI 的关键 |
| 通知去抖 | 30s 冷却 + 状态 hash 去重 + 首次循环 seed 不通知 | 直接复用；尤其"首次循环不通知"很关键，否则 app 启动瞬间会轰炸 |

### 6.3 Popover（NSPanel）

cc-view 如果要做 tray popover，**直接复制 lib.rs 中的 NSPanel setup 代码块**，关键配置一个不能少：

1. `PanelLevel::Status` (25) — 和菜单栏同级，浮在普通窗口之上
2. `StyleMask::nonactivating_panel` — **不抢焦点**（这是 cc-view "总览不打断当前会话"的硬需求）
3. `CollectionBehavior: full_screen_auxiliary | can_join_all_spaces | stationary` — 全屏 app 之上可见、切 Space 不消失
4. `hides_on_deactivate(false)` — 切到别的 app 不消失
5. `window_did_resign_key` → hide — 点击外部自动收
6. tray Left/Up 事件里用 `rect.position` 计算 popover 位置（不是用鼠标位置）
7. tauri.conf.json 里 `visible: false` + `decorations: false` + `transparent: true` + `skipTaskbar: true`

**cc-view 必须做的额外工作**：c9watch 是 Svelte，cc-view 是 Vue，`/popover` 路由的实现要改；NSPanel Rust 侧代码可几乎原样照搬。

### 6.4 其他可抄的小招

- **诊断事件**：`claude_processes_found > 0 && processes_with_cwd == 0` 即提示用户开 Full Disk Access——cc-view 必然会遇到同样问题，建议第一天就做这个诊断
- **`encode_path_for_matching`**：`chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })`——和 Claude Code 的 project_dir 编码完全对齐，cc-view 必须用同样规则才能正确匹配
- **Debug 日志面板** (`Cmd+Shift+D`)：内存环缓冲，对排查"为什么某个会话没被发现"极其有用
- **is_system_content**：识别 `<bash-stdout>` / `<bash-stderr>` / `<command-name>` 等 Claude Code 注入的系统消息，避免它们污染 first_prompt 和 latest_message 预览

---

## 7. 踩坑 / 注意事项

| 坑 | 表现 | c9watch 的应对 |
|----|------|----------------|
| **Full Disk Access** | sysinfo 能列出 claude 进程但 `cwd()` 永远是 None | 启动时算 `fda_likely_needed` 诊断标志，前端高亮提示用户去开 FDA |
| **npm 安装的 Claude Code 进程名是 "node"** | `process.name().contains("claude")` 漏掉所有 npm 用户 | 必须额外检查 `process.cmd()` 的 args 数组 |
| **`/clear` 后 session_id 变化** | 旧的 `sessions/<pid>.json` 会被覆盖，但 JSONL 文件还在 | 主路径每次都重新读 pid.json，拿到的是最新 session_id |
| **session_id 一对多 project_dir** | 同一个 session 可能因为某种原因在多个目录下有 JSONL | 用 `used_session_ids: HashSet` 去重 |
| **新进程还没写 JSONL** | 进程刚起 1s 内 JSONL 还没出现 | 路径编码匹配时强制 `session_mtime + 5s >= proc.start_time`，避免误匹配老 session |
| **pipe deadlock** | `claude agents --json` 输出 >64KB 时阻塞 | 必须 spawn 后台线程 drain stdout，主线程 wait_timeout |
| **subprocess zombie** | kill 后不 wait 会留 zombie | kill 之后必须再 wait() 一次 reap |
| **Mutex poison** | polling 线程 panic 后锁被污染 | `match detector.lock() { Ok(g) => g, Err(poisoned) => poisoned.into_inner() }` 强制恢复 |
| **stop_reason 永远是 None** | Claude Code 的 JSONL 不写 stop_reason 字段（API 返回才有） | 不能依赖 stop_reason 判完成，必须结合时间戳和文件 mtime |
| **通知在 dev 模式不出现** | `tauri-plugin-notification` 在 dev build 下可能无声 | 源码注释明确说明，prod 才稳定 |
| **窗口全关 = app 退出** | menubar 应用的大忌 | `CloseRequested::api.prevent_close()` + `ExitRequested::api.prevent_exit()` 双保险 |
| **主窗口 focus 时锁面板** | 主窗口抢焦点触发 panel 失焦隐藏，hide 时和 panel manager mutex 死锁 | 注释明确：macOS 上**不要**在 `show_main_window` 里调 `panel.hide()`，让 `window_did_resign_key` 异步处理 |
| **PermissionChecker 不热更** | 用户运行中改 `~/.claude/settings.json` 后，旧的 auto-approve 判断仍然生效直到 app 重启 | 用 `OnceLock` 是性能考量；cc-view 可以加 mtime 检查或每次 poll 时轻量重载 |
| **legacy 空 JSONL 幽灵 PID** | Legacy 后端匹配到 PID 但 JSONL 不存在 / 空 | enrichment 显式跳过 `message_count == 0 && !cli_sourced` 的 session |

---

## 8. 关键源文件链接

所有链接指向 `main` 分支 raw 内容，方便直接 curl：

| 文件 | 链接 | 重要性 |
|------|------|--------|
| session/status.rs | https://raw.githubusercontent.com/minchenlee/c9watch/main/src-tauri/src/session/status.rs | ★★★★★ 状态机核心，直接抄 |
| session/permissions.rs | https://raw.githubusercontent.com/minchenlee/c9watch/main/src-tauri/src/session/permissions.rs | ★★★★★ 等权限判定，直接抄 |
| session/detector.rs | https://raw.githubusercontent.com/minchenlee/c9watch/main/src-tauri/src/session/detector.rs | ★★★★ Legacy 进程发现 + 匹配 |
| session/detector_cli.rs | https://raw.githubusercontent.com/minchenlee/c9watch/main/src-tauri/src/session/detector_cli.rs | ★★★★ CLI 后端（CC ≥ 2.1.145） |
| session/enrichment.rs | https://raw.githubusercontent.com/minchenlee/c9watch/main/src-tauri/src/session/enrichment.rs | ★★★★ Session 结构 + mtime 兜底 |
| session/parser.rs | https://raw.githubusercontent.com/minchenlee/c9watch/main/src-tauri/src/session/parser.rs | ★★★ JSONL 解析 + 尾部读取 |
| polling.rs | https://raw.githubusercontent.com/minchenlee/c9watch/main/src-tauri/src/polling.rs | ★★★★ 事件流 + 通知去抖 + hash 去重 |
| lib.rs | https://raw.githubusercontent.com/minchenlee/c9watch/main/src-tauri/src/lib.rs | ★★★★★ NSPanel setup + tray + commands |
| session/state.rs | https://raw.githubusercontent.com/minchenlee/c9watch/main/src-tauri/src/session/state.rs | ★★★ 自动降级 + 重新聚焦重探 |
| session/mod.rs | https://raw.githubusercontent.com/minchenlee/c9watch/main/src-tauri/src/session/mod.rs | ★★★ 工厂 + 版本探测 |
| tauri.conf.json | https://raw.githubusercontent.com/minchenlee/c9watch/main/src-tauri/tauri.conf.json | ★★★ popover 窗口声明 |
| README.md | https://raw.githubusercontent.com/minchenlee/c9watch/main/README.md | ★★ 总览 |

---

## 附：未查到 / 不确定

- **actions.rs** 的具体实现（open_session 如何 focus 到对应终端）未深读，只确认存在 `open_session(pid, project_path)` 和 `stop_session(pid)` 命令。cc-view 需要的"一键 focus 会话窗口"建议后续单独抓这个文件看 AppleScript / `osascript` 调用细节。
- **`~/.claude/daemon/roster.json`**：c9watch 完全没用这个文件，cc-view 设计中提到的这个数据源是否是 Claude Code 官方稳定接口未在 c9watch 仓库中得到验证，**建议 cc-view 优先验证其存在性和稳定性**，必要时改走 c9watch 的三路数据组合。
- **Vue 等价实现**：c9watch 是 Svelte 5，`listen('sessions-updated')` 的响应式 store 写法不能直接搬到 Vue。Vue 侧需要用 `@tauri-apps/api/event::listen` + `ref.value = payload`，或在 Pinia store 里包装。
- **tauri-nspanel crate 的 API 细节**：报告里的 `set_level` / `set_collection_behavior` 等 API 来自源码用法，未读 crate 文档，建议 cc-view 实施时直接参考 c9watch 的 lib.rs 而非 crate 官方文档（c9watch 是经过验证的可用范例）。
