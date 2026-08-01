# claude-status 深度调研报告

> 调研日期: 2026-08-01 | 实测版本: claude-status plugin v2.0.5 (commit 6a4ef500 / plugin 007e1820)
> 仓库: 主 app [gmr/claude-status](https://github.com/gmr/claude-status) + plugin [gmr/claude-status-plugin](https://github.com/gmr/claude-status-plugin)（作为 submodule `claude-status-plugin/` 引入主仓库）

## 1. 概述

**claude-status 是什么**：原生 macOS menubar 应用，零侵入监控本机所有 Claude Code 会话，点一下 session 就跳到承载它的精确 window/tab/pane。作者 Gavin M. Roy（[Poison Pen LLC](https://github.com/gmr)）。原生 Swift 实现（非 Tauri/Electron），app + WidgetKit 桌面 widget 双形态。

- **技术栈**:
  - **主 app**: Swift 5.0 (`SWIFT_APPROACHABLE_CONCURRENCY=YES`, `SWIFT_DEFAULT_ACTOR_ISOLATION=MainActor`) + SwiftUI + AppKit 混合（AppKit 管 `NSStatusItem`/`NSPopover`/`NSWindow`，SwiftUI 画所有 view）
  - **Widget**: WidgetKit（status / productivity / score 三种桌面 widget）
  - **Plugin**: **Rust**（workspace 三 crate: `session-status` / `set-session-name` / `jsonl-analyzer`），编译成单 binary `plugins/claude-status/scripts/session-status`
  - **SPM 依赖**: Sparkle 2.9.0 (EdDSA 签名自动更新) / CocoaLumberjack 3.9.0 / swift-log 1.10.1
- **License**: BSD-3-Clause
- **macOS 版本**: README 标 **macOS 26.2+**；CI 中 `MACOSX_DEPLOYMENT_TARGET` override 到 **15.0**（兼容老 Xcode）；要求 Xcode 26 才能从源码编译
- **分发渠道**: **不在 App Store**，Developer ID 签名 + 公证 + Sparkle appcast 自动更新（EdDSA 签名的 appcast.xml 托管在 `gh-pages`）
- **关键 Bundle 配置**:
  - App Bundle ID: `com.poisonpenllc.Claude-Status` / Widget: `com.poisonpenllc.Claude-Status.widget`
  - App Group: `group.com.poisonpenllc.Claude-Status`（app 与 widget 共享 `sessions.json` / `productivity.json`）
  - URL Scheme: `claude-status://session/<id>`（widget 点击 → 唤起 app → focus session）
  - **`LSUIElement = YES`**（无 Dock 图标，纯 menubar）
  - **无 App Sandbox**（entitlements 只有 `com.apple.security.automation.apple-events` + App Groups）— **禁用 sandbox 是硬性要求**，因为要用 `proc_pidinfo` / `sysctl KERN_PROCARGS2` / AppleScript 自动化

### 整体数据流

```
Claude Code 进程
  │
  │ ① 触发 hook（SessionStart / PermissionRequest / Notification / PreCompact / SessionEnd）
  ▼
session-status 二进制（Rust，4 hook 调用 + 1 个长生命周期 daemon）
  │
  ├── 模式 A: hook 模式（SessionStart）→ spawn daemon → 立刻退出
  ├── 模式 B: signal 模式（--signal）→ 写 .csignal 文件 → 退出
  ├── 模式 C: daemon 模式（--daemon）→ tail JSONL → 维护状态机 → 写 .cstatus
  └── 模式 D: hook 模式（SessionEnd）→ 清理 .cstatus/.csignal/.cpid/.clog
  │
  ▼
.cstatus JSON 文件 + Darwin notification (com.poisonpenllc.Claude-Status.session-changed)
  │
  ▼
Swift menubar app（三重接收：Darwin 即时 / DispatchSource 文件监听 / 5s 轮询兜底）
  │
  ├── 进程树遍历 + 读环境变量 → 分类 source（iTerm2 / Terminal / VS Code / ... / tmux）
  ├── 读 ITERM_SESSION_ID / TMUX_PANE / TMUX → 精确 tab/pane 定位信息
  └── 用户点击 → TerminalFocuser → AppleScript / tmux select-pane / NSRunningApplication.activate()
```

---

## 2. 窗口 focus 机制（最关键，分终端列策略）

### 2.1 总体策略：focus 的输入不是 pid 本身，而是**预解析好的定位元数据**

**这是整个 focus 设计的核心洞察**：当用户点击 session 时，可用的定位信息不是"从 pid 现场查 window"，而是 SessionDiscovery 阶段（每 5s 或通知到来时）就读好的三个字段，存在 `ClaudeSession` 里：

| 字段 | 来源环境变量 | 用途 |
|---|---|---|
| `iTermSessionId` | `ITERM_SESSION_ID`（格式 `w0t0p0:<UUID>`）| iTerm2 AppleScript 用 UUID 精确匹配 session → tab → window |
| `tmuxPaneId` | `TMUX_PANE`（如 `%5`） | `tmux select-pane -t %5` + `select-window -t %5` |
| `tmuxSocket` | `TMUX`（格式 `/tmp/tmux-501/default,1234,0`） | `tmux -S <socket> ...`（非默认 socket 实例）|
| `workingDirectory` | `cwd` from `.cstatus` | Ghostty AppleScript 按 cwd 匹配 terminal（fallback） |

**`ClaudeSession` 数据模型**（`Shared/ClaudeSession.swift`）：

```swift
struct ClaudeSession: Identifiable, Codable, Equatable {
    let sessionId: String          // Claude Code 的 UUID（SwiftUI identity，稳定）
    let pid: pid_t
    let workingDirectory: String
    let state: SessionState
    let lastActivityAt: Date
    let iTermSessionId: String?    // ← focus 关键字段
    let tmuxPaneId: String?        // ← focus 关键字段
    let tmuxSocket: String?        // ← focus 关键字段
    let source: SessionSource       // .terminal(app:) / .xcode / .vscode / .jetbrains(ide:) / .zed
    let activity: String
    let sessionName: String?
    var profileName: String? = nil
}

enum SessionSource: Codable, Equatable {
    case terminal(app: String)      // "iTerm2", "Terminal", "Ghostty", ...
    case xcode
    case vscode
    case jetbrains(ide: String)     // "PyCharm", "IntelliJ IDEA", ...
    case zed
}
```

### 2.2 进程 → 终端 app 的映射逻辑（SessionDiscovery.swift 的 classifySource）

**关键 know-how**：从 `.cstatus` 拿到 pid + ppid，然后**走三层判定**：

1. **Claude 自己的可执行路径**（IDE 内嵌 binary 场景）→ `proc_pidpath(pid)`：
   - 含 `/Developer/Xcode/CodingAssistant/` → `.xcode`
   - 含 `.vscode/extensions/anthropic.claude-code` → `.vscode`
2. **Claude 自己的环境变量**（IDE agent 场景）→ `sysctl KERN_PROCARGS2` 读：
   - `TERMINAL_EMULATOR` 以 `JetBrains` 开头 → `.jetbrains(ide: ...)`（IDE 名从 `__CFBundleIdentifier` 末段映射）
   - `TERM_PROGRAM == "Zed"` → `.zed`
3. **父进程链往上爬**（最多 8 层），每层用 `proc_pidpath` 看路径含哪个 `.app`：

```swift
// Walk the ancestor chain starting from ppid
var current = ppid
for _ in 0..<8 {
    guard current > 1 else { break }
    if let path = executablePath(for: current) {
        // IDEs
        if path.contains("/Zed.app/") || path.contains("/zed-editor") { return .zed }
        if path.contains("/Visual Studio Code.app/") || path.contains("/Code.app/") { return .vscode }
        // Terminals
        if path.contains("/iTerm2.app/") || path.contains("/iTerm.app/") { return .terminal(app: "iTerm2") }
        if path.contains("/Terminal.app/") { return .terminal(app: "Terminal") }
        if path.contains("/Warp.app/") { return .terminal(app: "Warp") }
        if path.contains("/Alacritty.app/") { return .terminal(app: "Alacritty") }
        if path.contains("/kitty.app/") || path.contains("/Kitty.app/") { return .terminal(app: "Kitty") }
        if path.contains("/WezTerm.app/") || path.contains("/wezterm") { return .terminal(app: "WezTerm") }
        if path.contains("/Ghostty.app/") { return .terminal(app: "Ghostty") }
    }
    if let name = processName(for: current), name == "zed" { return .zed }
    guard let nextPid = parentPid(for: current) else { break }
    current = nextPid
}
```

4. **tmux 特殊处理**（最棘手）：tmux server 会被 reparent 到 pid 1，祖先链爬不到外层终端。所以如果 `TMUX` 环境变量存在 → 走一套**纯环境变量推断**：

```swift
// tmux server reparented to pid 1，靠 IDE 环境变量判断
if readEnvironmentVariable(for: pid, name: "TMUX") != nil {
    if readEnvironmentVariable(for: pid, name: "VSCODE_GIT_IPC_HANDLE") != nil { return .vscode }
    if let termProgram = readEnvironmentVariable(for: pid, name: "TERM_PROGRAM"),
       termProgram == "Zed" { return .zed }
    return .terminal(app: resolveTerminalFromTmux(pid: pid))
}

// TERM_PROGRAM 被 tmux 覆盖成 "tmux"，所以查这些存活到 tmux session 里的 env：
private func resolveTerminalFromTmux(pid: pid_t) -> String {
    if let lcTerminal = readEnvironmentVariable(for: pid, name: "LC_TERMINAL") {
        if lcTerminal.contains("iTerm") { return "iTerm2" }
        return lcTerminal
    }
    if readEnvironmentVariable(for: pid, name: "ITERM_SESSION_ID") != nil { return "iTerm2" }
    if readEnvironmentVariable(for: pid, name: "GHOSTTY_RESOURCES_DIR") != nil { return "Ghostty" }
    if readEnvironmentVariable(for: pid, name: "KITTY_PID") != nil { return "Kitty" }
    if readEnvironmentVariable(for: pid, name: "WEZTERM_PANE") != nil { return "WezTerm" }
    if readEnvironmentVariable(for: pid, name: "ALACRITTY_SOCKET") != nil { return "Alacritty" }
    // ... fallback to TERM_PROGRAM
}
```

5. **最终 fallback**：`TERM_PROGRAM` 环境变量（`iTerm.app` → iTerm2 / `Apple_Terminal` → Terminal / `WarpTerminal` → Warp / `ghostty` → Ghostty）。

### 2.3 进程信息 API（Swift 端的 libproc / sysctl 调用）

```swift
// 可执行路径：proc_pidpath
private func executablePath(for pid: pid_t) -> String? {
    var buffer = [CChar](repeating: 0, count: Int(MAXPATHLEN))
    let result = proc_pidpath(pid, &buffer, UInt32(buffer.count))
    guard result > 0 else { return nil }
    return String(cString: buffer)
}

// 进程名：proc_name
private func processName(for pid: pid_t) -> String? {
    var buffer = [CChar](repeating: 0, count: Int(MAXPATHLEN))
    let result = proc_name(pid, &buffer, UInt32(buffer.count))
    guard result > 0 else { return nil }
    return String(cString: buffer)
}

// 父 pid：proc_pidinfo + PROC_PIDTBSDINFO，读 proc_bsdinfo.pbi_ppid
private func parentPid(for pid: pid_t) -> pid_t? {
    var info = proc_bsdinfo()
    let size = Int32(MemoryLayout<proc_bsdinfo>.size)
    let result = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size)
    guard result == size else { return nil }
    let ppid = pid_t(info.pbi_ppid)
    return ppid > 1 ? ppid : nil
}

// 读其他进程的环境变量：sysctl KERN_PROCARGS2（最关键的工具）
// 流程：先拿 KERN_ARGMAX → 分配缓冲 → sysctl KERN_PROCARGS2,pid →
// 跳过 [argc:Int32][exe path][padding][argv strings] → 扫描 env 区
func readEnvironmentVariable(for pid: pid_t, name: String) -> String? {
    var argmax: Int32 = 0
    var mib: [Int32] = [CTL_KERN, KERN_ARGMAX]
    var size = MemoryLayout<Int32>.size
    guard sysctl(&mib, 2, &argmax, &size, nil, 0) == 0, argmax > 0 else { return nil }

    var procargs = [UInt8](repeating: 0, count: Int(argmax))
    mib = [CTL_KERN, KERN_PROCARGS2, pid]
    size = Int(argmax)
    guard sysctl(&mib, 3, &procargs, &size, nil, 0) == 0, size > 0 else { return nil }

    var offset = MemoryLayout<Int32>.size
    // Skip executable path
    while offset < size && procargs[offset] != 0 { offset += 1 }
    while offset < size && procargs[offset] == 0 { offset += 1 }
    // Skip argv
    let argc = procargs.withUnsafeBytes { $0.load(as: Int32.self) }
    for _ in 0..<argc {
        while offset < size && procargs[offset] != 0 { offset += 1 }
        offset += 1
    }
    // Scan env
    let searchKey = name + "="
    while offset < size {
        let start = offset
        while offset < size && procargs[offset] != 0 { offset += 1 }
        if offset > start {
            let envString = String(bytes: procargs[start..<offset], encoding: .utf8) ?? ""
            if envString.hasPrefix(searchKey) {
                let value = String(envString.dropFirst(searchKey.count))
                return value.isEmpty ? nil : value
            }
        }
        offset += 1
    }
    return nil
}
```

**关键提示**：Rust 端 `session-status` 也实现了同样的 `get_ppid_of`（直接 FFI `proc_pidinfo`，从 136 字节的 `PROC_PIDTBSDINFO` buffer 偏移 24 读 u32 ppid），可直接复用到 cc-view。

### 2.4 各终端的 focus 实现（TerminalFocuser.swift）

#### 2.4.1 分流入口（按 source 枚举）

```swift
struct SessionFocuser {
    func focus(session: ClaudeSession) {
        switch session.source {
        case .terminal(let app):
            focusTerminal(app: app,
                sessionId: session.iTermSessionId,
                tmuxPaneId: session.tmuxPaneId,
                tmuxSocket: session.tmuxSocket,
                workingDirectory: session.workingDirectory)
        case .xcode:    activateApp(bundleId: "com.apple.dt.Xcode")
        case .vscode:   activateApp(bundleId: "com.microsoft.VSCode")
        case .jetbrains: activateJetBrainsApp()  // 任何 com.jetbrains.* 前缀
        case .zed:      activateApp(bundleId: "dev.zed.Zed")
        }
    }
}
```

#### 2.4.2 各终端策略一览

| 终端 | 精确度 | 机制 |
|---|---|---|
| **iTerm2** | **精确 tab + window** | AppleScript 遍历 `windows → tabs → sessions` 匹配 `unique ID` |
| **Ghostty** | **精确 terminal + window** | AppleScript 按 `working directory` 匹配 terminal（路径归一化解 symlink）|
| **tmux（任何外层终端）** | **精确 pane + window** | `tmux select-window` + `select-pane -t <paneId>` + zoom 检测，然后外层 terminal activate |
| **Terminal / Warp / Alacritty / Kitty / WezTerm** | 仅 app 级 | `NSRunningApplication.activate()`（无法精确定位 tab/pane）|
| **VS Code / Xcode / Zed / JetBrains** | 仅 app 级 | `NSRunningApplication.activate()` |

#### 2.4.3 iTerm2 精确 focus 的 AppleScript（最值得抄）

iTerm2 把 `ITERM_SESSION_ID` 暴露成 `w0t0p0:<UUID>` 格式，UUID 等于 AppleScript 里 session 的 `unique ID` 属性。先 select tab，再 select window，再 activate（顺序很重要——先选好 tab 让 iTerm 把对应 window 推到前面）：

```swift
private func focusBySessionId(_ sessionId: String) {
    // ITERM_SESSION_ID = "w0t0p0:UUID" — 提取 UUID
    let uniqueId: String
    if let colonIndex = sessionId.firstIndex(of: ":") {
        uniqueId = String(sessionId[sessionId.index(after: colonIndex)...])
    } else {
        uniqueId = sessionId
    }
    // 防 AppleScript 注入（只允许字母数字+连字符）
    let range = NSRange(uniqueId.startIndex..., in: uniqueId)
    guard Self.safeIdPattern.firstMatch(in: uniqueId, range: range) != nil else { return }

    let script = """
    tell application "iTerm2"
        repeat with aWindow in windows
            repeat with aTab in tabs of aWindow
                repeat with aSession in sessions of aTab
                    if unique ID of aSession is "\(uniqueId)" then
                        select aTab
                        tell aWindow
                            select
                        end tell
                        activate
                        return
                    end if
                end repeat
            end repeat
        end repeat
    end tell
    """
    runAppleScript(script)
}
```

**没拿到 sessionId 但有 cwd 的 fallback**：开新 tab + `cd` 到 cwd：

```applescript
tell application "iTerm2"
    activate
    tell current window
        create tab with default profile
        tell current session
            write text "cd \"<escapedDir>\""
        end tell
    end tell
end tell
```

#### 2.4.4 Ghostty 按 cwd 匹配的 AppleScript

Ghostty 的 AppleScript 暴露 `working directory of terminal`，所以**按 cwd 反向匹配**。路径归一化用 `(POSIX file tDir) as alias` 解 symlink（重要——`~/Source/...` 和 `/Users/.../Source/...` 才能对得上）：

```swift
private func focusGhosttyTerminal(workingDirectory: String) {
    let escapedDir = appleScriptEscape(workingDirectory)
    let script = """
    tell application "Ghostty"
        set targetDir to "\(escapedDir)"
        try
            set normalTarget to POSIX path of ((POSIX file targetDir) as alias)
        on error
            set normalTarget to targetDir
        end try
        repeat with aWindow in windows
            repeat with t in every terminal of aWindow
                set tDir to working directory of t
                try
                    set normalTDir to POSIX path of ((POSIX file tDir) as alias)
                on error
                    set normalTDir to tDir
                end try
                if normalTDir is normalTarget then
                    focus t
                    set index of aWindow to 1   -- 窗口置顶
                    activate
                    return
                end if
            end repeat
        end repeat
        activate
    end tell
    """
    runAppleScript(script)
}
```

#### 2.4.5 tmux pane focus（Process 调 tmux 二进制，不走 AppleScript）

```swift
// 解析 tmux 路径：Homebrew / MacPorts / fallback /usr/bin/env tmux
private static let tmuxPath: String = {
    for candidate in ["/opt/homebrew/bin/tmux", "/usr/local/bin/tmux", "/opt/local/bin/tmux"] {
        if FileManager.default.isExecutableFile(atPath: candidate) { return candidate }
    }
    return "/usr/bin/env"
}()

private func focusTmuxPane(paneId: String, socket: String?) {
    var baseArgs = [String]()
    if let socket { baseArgs += ["-S", socket] }   // 非默认 socket 实例
    let tmuxBin = Self.tmuxPath
    let usesEnv = tmuxBin == "/usr/bin/env"

    // 1. 选 pane 所在的 window
    let selectWindow = Process()
    selectWindow.executableURL = URL(fileURLWithPath: tmuxBin)
    selectWindow.arguments = (usesEnv ? ["tmux"] : []) + baseArgs + ["select-window", "-t", paneId]
    try? selectWindow.run(); selectWindow.waitUntilExit()

    // 2. 如果当前 window 有 pane 处于 zoom（全屏），先解除 zoom（resize-pane -Z 是 toggle，
    //    所以先查 window_zoomed_flag 决定要不要 toggle，避免反向把 pane zoom 了）
    let checkZoom = Process()
    let pipe = Pipe()
    checkZoom.executableURL = URL(fileURLWithPath: tmuxBin)
    checkZoom.arguments = (usesEnv ? ["tmux"] : []) + baseArgs + [
        "display-message", "-p", "#{window_zoomed_flag}"
    ]
    checkZoom.standardOutput = pipe
    try? checkZoom.run(); checkZoom.waitUntilExit()
    let zoomFlag = String(data: pipe.fileHandleForReading.readDataToEndOfFile(),
                          encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines)
    if zoomFlag == "1" {
        let unzoom = Process()
        unzoom.executableURL = URL(fileURLWithPath: tmuxBin)
        unzoom.arguments = (usesEnv ? ["tmux"] : []) + baseArgs + ["resize-pane", "-Z"]
        try? unzoom.run(); unzoom.waitUntilExit()
    }

    // 3. 选 pane 本身
    let selectPane = Process()
    selectPane.executableURL = URL(fileURLWithPath: tmuxBin)
    selectPane.arguments = (usesEnv ? ["tmux"] : []) + baseArgs + ["select-pane", "-t", paneId]
    try? selectPane.run(); selectPane.waitUntilExit()
}
```

**tmux + iTerm2 组合**：先 `focusTmuxPane` 选好 tmux 的 window/pane（让目标 pane 在 tmux 内可见），再用 iTerm AppleScript focus 承载这个 tmux session 的 iTerm tab/window：

```swift
if let paneId = tmuxPaneId {
    focusTmuxPane(paneId: paneId, socket: tmuxSocket)
    if app == "iTerm2", let sessionId {
        focusBySessionId(sessionId)   // 再聚焦 iTerm 的 tab
    } else {
        activateTerminalApp(name: app)
    }
    return
}
```

#### 2.4.6 通用 app activate（IDE / 无 AppleScript 终端）

```swift
private func activateApp(bundleId: String) {
    guard let app = NSRunningApplication.runningApplications(withBundleIdentifier: bundleId).first else { return }
    app.activate()
}

// JetBrains 特殊：多个 IDE 可能同时跑，前缀匹配
private func activateJetBrainsApp() {
    let jetbrainsApp = NSWorkspace.shared.runningApplications.first { app in
        guard let bundleId = app.bundleIdentifier else { return false }
        return bundleId.hasPrefix("com.jetbrains.")
    }
    jetbrainsApp?.activate()
}

// 终端 bundle ID 表（用于 fallback 的 name → bundle ID 映射）
private static let terminalBundleIds: [String: String] = [
    "iTerm2":    "com.googlecode.iterm2",
    "Terminal":  "com.apple.Terminal",
    "Warp":      "dev.warp.Warp-Stable",
    "Alacritty": "org.alacritty",
    "Kitty":     "net.kovidgoyal.kitty",
    "WezTerm":   "com.github.wez.wezterm",
    "Ghostty":   "com.mitchellh.ghostty",
]
```

### 2.5 进程存活验证（防 PID 回收错判）

`SessionDiscovery` 在每次扫描时验证 pid 还活着**且可执行路径仍含 claude/Claude/node**，避免 PID 被回收给无关进程：

```swift
private func isProcessAlive(_ pid: pid_t) -> Bool {
    guard kill(pid, 0) == 0 else { return false }
    guard let path = executablePath(for: pid) else { return true }
    return path.contains("claude") || path.contains("Claude") || path.hasSuffix("/node")
}
```

死掉的 session 加入 `deadSessions: Set<String>`，后续扫描跳过，直到 Darwin 通知到来 `clearDeadSessions()`。

---

## 3. Plugin Hook + .cstatus 格式

### 3.1 Hooks 注册（极简——4 个 hook，不是 14 个）

> **重要发现**：README 和老版 CLAUDE.md 提到"14 hook events / Python session-status.py"，但 **2026 重写为 Rust 后只注册 4 个 hook**（plugin 自己的 CLAUDE.md 明确写"down from 12 in the Python version"）。原因是引入了**长生命周期 daemon**，绝大多数状态变化由 daemon tail JSONL 自己推断，不再需要 PreToolUse / PostToolUse / Stop / SubagentStop 等 hook。

`plugins/claude-status/hooks/hooks.json` 全文：

```json
{
  "hooks": {
    "SessionStart":         [{ "hooks": [{ "type": "command", "command": "\"${CLAUDE_PLUGIN_ROOT}/scripts/session-status\"", "timeout": 5 }] }],
    "PermissionRequest":    [{ "hooks": [{ "type": "command", "command": "\"${CLAUDE_PLUGIN_ROOT}/scripts/session-status\" --signal", "timeout": 5 }] }],
    "Notification":         [{ "hooks": [{ "type": "command", "command": "\"${CLAUDE_PLUGIN_ROOT}/scripts/session-status\" --signal", "timeout": 5 }] }],
    "PreCompact":           [{ "hooks": [{ "type": "command", "command": "\"${CLAUDE_PLUGIN_ROOT}/scripts/session-status\" --signal", "timeout": 5 }] }],
    "SessionEnd":           [{ "hooks": [{ "type": "command", "command": "\"${CLAUDE_PLUGIN_ROOT}/scripts/session-status\"", "timeout": 5 }] }]
  }
}
```

| Hook | 模式 | 用途 |
|---|---|---|
| `SessionStart` | hook（无 flag） | 读 stdin JSON，写初始 `.cstatus`，spawn daemon（独立进程组 `process_group(0)`，hook runner 杀不掉），发 Darwin 通知，立刻退出 |
| `SessionEnd` | hook | 安全网清理：删 `.cstatus` / `.csignal` / `.cpid` / `.clog`，发 Darwin 通知（daemon 自己挂了也能清场）|
| `PermissionRequest` | signal（`--signal`） | 写 `.csignal` `{type:"permission_request",tool_name:"..."}` 给 daemon（UI 弹权限框 = Waiting）|
| `Notification` | signal | notification_type → `{permission_prompt\|elicitation_dialog\|idle_prompt}` 信号 |
| `PreCompact` | signal | `{type:"pre_compact"}` 信号（让 daemon 在 compact_boundary 到来前就置 Compacting）|

### 3.2 Hook stdin 数据格式（Claude Code → plugin）

SessionStart / SessionEnd 模式读到的 JSON 字段：
- `session_id` — UUID
- `transcript_path` — `<project_dir>/<session_id>.jsonl`（**.cstatus 就写在这个路径旁边**，替换扩展名）
- `cwd` — 当前工作目录
- `hook_event_name` — `"SessionStart"` / `"SessionEnd"` / `"PermissionRequest"` / `"Notification"` / `"PreCompact"`

Signal 模式额外字段：
- `PermissionRequest`: `tool_name`
- `Notification`: `notification_type` (`permission_prompt` / `elicitation_dialog` / `idle_prompt`)

**PID 来源**：优先 `CLAUDE_PID` 环境变量，否则 `std::os::unix::process::parent_id()`（plugin 进程的父进程 = Claude CLI 进程）：

```rust
let pid: u32 = env::var("CLAUDE_PID")
    .ok().and_then(|v| v.parse().ok())
    .unwrap_or_else(parent_id);
let ppid = get_ppid_of(pid).unwrap_or(0);
```

### 3.3 `.cstatus` 文件格式

**路径**：`~/.claude/projects/<encoded-project-path>/<session-id>.cstatus`（与 `<session-id>.jsonl` transcript 同目录，扩展名替换）。
**多 profile**：如果用户用 `CLAUDE_CONFIG_DIR=~/.claude-work`，则路径是 `~/.claude-work/projects/.../`。ProfileStore 自动探测 `~/.claude-*` 目录。

**内容**（单行 JSON，原子写：tempfile + rename）：

```json
{"session_id":"<uuid>","pid":12345,"ppid":12340,"state":"active","activity":"Bash","timestamp":"2026-08-01T18:14:38Z","cwd":"/Users/foo/proj","event":"assistant","session_name":"API Refactor"}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `session_id` | string | UUID（Claude Code 提供） |
| `pid` | int | Claude Code 进程 pid |
| `ppid` | int | Claude Code 的父 pid（用于进程树分类）|
| `state` | string | `active` / `waiting` / `idle` / `compacting` |
| `activity` | string | 当前活动描述：`thinking` / 工具名（`Bash` / `Read` / `subagent` / `mcp` / `question` / `compacting` / `compacting (Read)`）/ 空 |
| `timestamp` | string | ISO8601 UTC（秒精度，`gmtime_r` 格式化）|
| `cwd` | string | 工作目录（来自 hook payload）|
| `event` | string | 触发本次写入的事件类型（`SessionStart` / `user` / `assistant` / `progress:bash_progress` / `system:compact_boundary` / `permission_request` / `idle_prompt` / ...）|
| `session_name` | string? | 可选，用户通过 `/name-session` 设置，跨状态写入保留 |

**`.csignal` 格式**（一次性，daemon 读后删）：`{"type":"permission_request\|elicitation_dialog\|idle_prompt\|pre_compact","tool_name":"..."}`

**`.cpid` 文件**：daemon 启动时写自己的 pid，下次 SessionStart 时检测到 daemon 还活着就不重新 spawn（session resume 场景）。

**`.clog` 文件**：daemon 的 stderr 重定向到这里（崩溃调试用）。

### 3.4 SessionStart → spawn daemon 的关键代码

```rust
fn hook_session_start(input: &Value) -> Result<(), String> {
    let session_id = input.get("session_id")...;
    let transcript_path = input.get("transcript_path")...;
    let cwd = input.get("cwd")...;
    let pid: u32 = env::var("CLAUDE_PID").ok().and_then(|v| v.parse().ok()).unwrap_or_else(parent_id);
    let ppid = get_ppid_of(pid).unwrap_or(0);

    let cstatus = transcript_sibling(transcript_path, "cstatus");
    if let Some(dir) = cstatus.parent() { fs::create_dir_all(dir)?; }

    // 写初始 .cstatus（state=idle, event=SessionStart）
    write_atomic(&cstatus, status.to_json().as_bytes())?;

    // 检测已有 daemon（session resume）
    let cpid_path = transcript_sibling(transcript_path, "cpid");
    if let Ok(pid_str) = fs::read_to_string(&cpid_path)
        && let Ok(daemon_pid) = pid_str.trim().parse::<u32>()
        && pid_is_alive(daemon_pid)
    {
        post_darwin_notification();
        return Ok(());
    }

    // spawn daemon — 关键：process_group(0) 让 daemon 独立进程组，
    // hook runner 后续 SIGTERM/退出不会拖死 daemon
    let exe = env::current_exe()?;
    Command::new(exe)
        .arg("--daemon")
        .arg("--transcript-path").arg(transcript_path)
        .arg("--session-id").arg(session_id)
        .arg("--cwd").arg(cwd)
        .arg("--pid").arg(pid.to_string())
        .arg("--ppid").arg(ppid.to_string())
        .stdin(Stdio::null()).stdout(Stdio::null())
        .stderr(stderr_cfg)  // 重定向到 .clog
        .process_group(0)
        .spawn()?;
    post_darwin_notification();
    Ok(())
}
```

---

## 4. Darwin Notification 三重接收机制

### 4.1 发送端（Rust，FFI `notify_post`）

```rust
unsafe extern "C" {
    fn notify_post(name: *const std::ffi::c_char) -> u32;
}

const NOTIFICATION_NAME: &str = "com.poisonpenllc.Claude-Status.session-changed";

fn post_darwin_notification() {
    let name = CString::new(NOTIFICATION_NAME).expect("invalid notification name");
    unsafe { notify_post(name.as_ptr()); }
}
```

**何时发**：daemon 每次状态变化写完 `.cstatus` 之后；SessionStart spawn 完 daemon；SessionEnd 清理完；set-session-name 改完 name。

> **设计要点**：用 FFI 直调 `notify_post` 而不是 fork `notifyutil -p`，避免每次状态变化都 fork/exec 一个进程（高频时开销显著）。README 提到 `notifyutil -p` 是早期描述，实际实现是 FFI。

### 4.2 接收端（Swift，CFNotificationCenter Darwin 中心）

```swift
@Observable @MainActor
final class SessionMonitor {
    private static let darwinNotificationName = "com.poisonpenllc.Claude-Status.session-changed" as CFString

    private func registerDarwinNotification() {
        let center = CFNotificationCenterGetDarwinNotifyCenter()  // 跨进程 Darwin 中心
        let observer = Unmanaged.passUnretained(self).toOpaque()  // self 指针桥接

        CFNotificationCenterAddObserver(
            center, observer,
            { _, observer, _, _, _ in
                guard let observer else { return }
                let monitor = Unmanaged<SessionMonitor>.fromOpaque(observer).takeUnretainedValue()
                DispatchQueue.main.async {  // callback 可能在任意线程，跳回主线程
                    monitor.refreshFromNotification()
                }
            },
            Self.darwinNotificationName,
            nil,
            .deliverImmediately  // 不等 run loop，立刻回调
        )
    }
}
```

### 4.3 三重接收的分工（SessionMonitor.start）

```swift
func start() {
    // ① 文件监听：DispatchSource 监听每个 profile 的 projects/ 目录变化
    stateResolver.onProjectsChanged = { [weak self] in self?.refresh() }
    updateWatchedDirectories()

    // ② Darwin 通知（即时推送）：clearDeadSessions + 全量扫
    registerDarwinNotification()

    // 启动时来一发
    refresh()

    // ③ 5s 轮询兜底：抓无 hook 的 session（IDE agent 等）
    timer = Timer.scheduledTimer(withTimeInterval: scanInterval, repeats: true) { [weak self] _ in
        self?.refresh()
    }
}
```

| 机制 | 触发延迟 | 用途 | 触发的 refresh |
|---|---|---|---|
| **Darwin 通知** | 毫秒级 | plugin hook 触发后的实时推送 | `refreshFromNotification()`（清 dead sessions + 全量扫） |
| **DispatchSource 文件监听** | ~秒级 | backup：抓 `.cstatus` 写入；也用于无 hook 场景下监听 JSONL 修改 | `refresh()` |
| **5s 轮询 timer** | 最多 5s | 兜底：抓完全没有 hook 的会话（IDE 内嵌 agent 不触发 plugin hook）；也是任何机制漏报的最终保险 | `refresh()` |

**DispatchSource 文件监听**细节（`StateResolver.swift`）：
- 对每个 profile 的 `projects/` 目录 `open(path, O_EVTONLY)` 拿 fd
- `DispatchSource.makeFileSystemObjectSource` 监听 `.write / .extend / .delete / .rename`
- 监听到变化就触发 `onProjectsChanged` callback（去重由上层 throttle 处理）
- `updateWatchedDirectories` 做集合 diff，新增目录加 watcher，移除的 cancel

### 4.4 refresh 流程（被三种机制殊途同归地调用）

```swift
func refresh() {
    refreshProfilesIfStale()   // 每 30s 重新探测 ~/.claude-* 目录
    let result = discovery.discoverAll(profiles: profileStore.enabledProfiles)
    applyResult(result)
}

private func applyResult(_ result: ...) {
    let sessionsChanged = sessions != result.sessions
    sessions = result.sessions
    cstatusCache = result.cstatusFiles
    tracker.recordSnapshot(sessions: result.sessions)  // 生产力统计
    updatePluginState()  // 每 30s 检查一次 plugin 是否装好
    if sessionsChanged {
        writeToSharedContainer()  // 写 App Group + reload WidgetCenter
    }
}
```

---

## 5. 状态机定义（四态判定，含 Compacting）

### 5.1 SessionState 枚举

```swift
enum SessionState: Comparable, Codable {
    case active       // ⚡ 🟢 Claude is working
    case waiting      // ⏳ 🟠 Needs your input
    case idle         // 💤 ⚪ No recent activity
    case compacting   // 🧹 🔵 Context compaction in progress

    // 聚合状态用 max(by: priority)：多 session 时取最紧急的
    var priority: Int {
        switch self {
        case .waiting: 3   // ← 用户输入最重要，置顶
        case .active: 2
        case .compacting: 1
        case .idle: 0
        }
    }
    // 列表排序：Waiting → Active → Compacting → Idle
    var sortOrder: Int { ... }
}
```

### 5.2 状态判定逻辑（daemon 的 JSONL 状态机，最值得抄）

**核心设计**：daemon 每 100ms tail 一次 JSONL transcript，对每行调用 `process_line` 更新 `DaemonState`。状态完全由 JSONL 推断，PermissionRequest/Notification/PreCompact 等 hook 信号通过 `.csignal` 文件旁路输入。

```rust
struct DaemonState {
    state: SessionState,
    activity: String,
    event: String,
    active_agents: HashSet<String>,    // 跟踪未完成的 subagent
    session_name: Option<String>,
    compacting: bool,                  // ← Compacting 抑制标志
}

fn process_line(&mut self, line: &str) -> bool {  // 返回状态是否变化
    // 跳过 isMeta 消息（local command caveat 等）
    if v.get("isMeta").as_bool() == true { return false; }

    match line_type {
        "assistant" => self.process_assistant(&v),
        "user"      => if self.compacting { /* 抑制 replayed context */ }
                       else { self.process_user(&v); },
        "progress"  => if self.compacting { /* 抑制 */ }
                       else { self.process_progress(&v); },
        "system"    => self.process_system(&v),
        _ => {}  // file-history-snapshot / last-prompt / pr-link / queue-operation 忽略
    }

    // acompact- agentId 检测（见 5.4）
    ...
}
```

### 5.3 Active / Waiting / Idle 判定

**Active** 的子状态（写入 `activity` 字段）：
| 触发 | activity |
|---|---|
| `assistant` + `stop_reason:null`（流式中）| 空 |
| `assistant` + `stop_reason:tool_use` | 最后一个 tool_use 的 `name`（如 `Bash`）|
| `user` + 有 `promptId` + text 内容 | `thinking` |
| `progress` + `data.type == agent_progress` | `subagent` |
| `progress` + `data.type == bash_progress` | `bash` |
| `progress` + `data.type == mcp_progress` | `mcp` |
| `end_turn` 但 `active_agents` 非空 | `subagent`（agents 还在跑）|

**Waiting** 判定：
1. `assistant` + `stop_reason:end_turn` + 最后一个 text block 的最后一段含 `?` → Waiting / activity=`question`
2. `.csignal` 收到 `permission_request` → Waiting / activity=tool_name
3. `.csignal` 收到 `elicitation_dialog` → Waiting

**Question 检测**（启发式，但有测试覆盖）：

```rust
fn detect_question(&self, content: &[Value]) -> bool {
    for block in content.iter().rev() {
        if block_type == "text" {
            let text = block.get("text").as_str();
            if text.is_empty() { continue; }
            // 只看最后一段（最后一个 \n\n 之后）
            let last_paragraph = text.rsplit("\n\n").next().trim();
            return last_paragraph.contains('?');
        }
    }
    false
}
```

**Idle** 判定：
- `end_turn` 且无活跃 agent 且非 question → Idle
- `stop_reason:max_tokens` → Idle（截断也视作本轮结束）
- `system:turn_duration` 系统消息（turn 真正结束的信号）→ 清理 stale agents + Active→Idle
- `.csignal` 收到 `idle_prompt` 且无活跃 agent → Idle

### 5.4 Compacting 判定（最有意思的部分）

**三种独立检测路径**（任一触发即置 Compacting）：

1. **`compact_boundary` 系统消息**（compaction 完成的边界标记）：

```rust
fn process_system(&mut self, v: &Value) {
    match subtype {
        "compact_boundary" => {
            self.compacting = true;       // ← 抑制标志
            self.state = SessionState::Compacting;
            self.activity = "compacting".to_string();
        }
        "turn_duration" => { ... },
        _ => {}
    }
}
```

2. **`agentId` 前缀 `acompact-`**（长 compaction 会 spawn 一个 compact agent）：

```rust
// 长会话压缩时，Claude 会启动 agentId = "acompact-xxxxxxxx" 的子 agent
if !self.compacting
    && self.state != SessionState::Idle
    && let Some(agent_id) = v.get("agentId").and_then(|a| a.as_str())
    && agent_id.starts_with("acompact-")
{
    self.compacting = true;
    self.state = SessionState::Compacting;
    self.activity = format!("compacting ({})", self.activity);  // 保留原 activity
}
```

3. **`PreCompact` hook 信号**（compact_boundary 到来前的预警）：

```rust
// hooks.json 注册的 PreCompact → --signal 模式 → 写 .csignal
fn process_signal(&mut self, signal: &Value) -> bool {
    match signal_type {
        "pre_compact" => {
            self.compacting = true;
            self.state = SessionState::Compacting;
            self.activity = "compacting".to_string();
        }
        ...
    }
}
```

**`compacting` 标志的关键作用**——抑制 replayed context：

> Compaction 完成后，Claude Code 会把压缩后的对话摘要作为新的 `user` 消息 replay 进 transcript（"This session is being continued..."）。如果不抑制，daemon 会误判为新用户提问 → 状态变 Active/thinking。所以 `compacting=true` 时所有 `user` 和 `progress` 消息都不更新状态，直到下一个真正的 `assistant` 响应到来（`process_assistant` 第一行就 `self.compacting = false`）。

```rust
fn process_assistant(&mut self, v: &Value) {
    // compaction 后的第一个 assistant 响应说明 replay 结束
    self.compacting = false;
    ...
}
```

实测场景（plugin 测试套件覆盖）：
- 手动 `/compact` → compact_boundary → replayed user/progress 被抑制 → assistant 响应 → Idle
- 长 compaction → acompact-agentId → compact_boundary → replayed → assistant → Idle
- `idle_prompt` 信号也能清掉 compacting 标志（compaction 后无 assistant 直接 idle 的边界情况）

---

## 6. 对 cc-view 的可借鉴点

### 6.1 Focus 机制如何移植到 Rust + osascript（最关键）

**整体结论**：claude-status 的 focus 逻辑 **95% 可以直接照抄到 cc-view 的 Rust 后端**，因为 plugin 本身已经是 Rust。具体路径：

#### 6.1.1 进程树遍历 / 读环境变量（直接复用 Rust FFI 代码）

cc-view 是 Tauri 2 + Rust 后端，正好可以把 plugin 仓库的 `crates/session-status/src/main.rs` 里的 `get_ppid_of`（FFI `proc_pidinfo` + offset 24 读 ppid）和主 app `SessionDiscovery.swift` 的 `readEnvironmentVariable`（`sysctl KERN_PROCARGS2`）翻成 Rust：

```rust
// 读其他进程环境变量（Rust 版本，可独立做成 crate 或内联）
// 流程：sysctl KERN_ARGMAX → sysctl KERN_PROCARGS2,pid → 跳过 argc/exe/argv → 扫 env
// 关键：用 nix crate 或 libc crate 提供的 sysctl / proc_pidinfo 绑定
```

需要的 Rust crate：`libc`（`proc_pidinfo`, `proc_pidpath`, `proc_name`, `sysctl` 都在 libc 绑定里）。`nix` 也可以但部分 API 没覆盖。

#### 6.1.2 AppleScript 调用（Rust → osascript 二进制）

Swift 端用 `NSAppleScript`，Rust 端最直接的方式是 `std::process::Command::new("osascript")` + `-e` 传脚本。iTerm2 / Ghostty 的 AppleScript 文本几乎不用改：

```rust
fn run_applescript(script: &str) -> io::Result<()> {
    let status = Command::new("osascript")
        .arg("-e").arg(script)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .status()?;
    Ok(())
}
```

如果想避免每次 fork `osascript`，可以用 `objc2` crate 调 `NSAppleScript`（更重但更快）——但对 cc-view 的使用频率（用户点击才触发）来说，fork `osascript` 完全够用。

#### 6.1.3 tmux / app activate（直接 Process / objc2）

- tmux：`Command::new(tmux_path).args(["select-window", "-t", pane_id])` —— 完全照抄 Swift 的 `focusTmuxPane`
- `NSRunningApplication.activate()`：Rust 端用 `objc2` + `objc2-app-kit`，或 fallback `open -a <app>`（不如 NSRunningApplication 精确）

#### 6.1.4 关键架构决策：focus 元数据何时采集

**强烈建议抄 claude-status 的设计**：不要在用户点击时**现场从 pid 查 window**（每次都要爬进程树 + 读 env var，慢且容易出错），而是**在 session 发现阶段就预解析** `iTermSessionId / tmuxPaneId / tmuxSocket / workingDirectory`，缓存到 session 模型里，点击时直接用。这样 focus 操作就是 O(1) 的 AppleScript/tmux 调用。

#### 6.1.5 cc-view focus 能力上界（实测能到哪）

照抄后 cc-view 能做到：
- **iTerm2 + tmux 组合**：精确到 pane（最强）
- **iTerm2 无 tmux**：精确到 tab + window
- **Ghostty**：按 cwd 精确匹配 terminal
- **Terminal / Warp / Alacritty / Kitty / WezTerm**：仅 app 级 activate（这些终端没暴露 AppleScript scripting API）
- **VS Code / Xcode / JetBrains / Zed**：仅 app 级

**cc-view 如果想突破"仅 app 级"**，可以：
- **Warp**：查 Warp 是否有命令行 / AppleScript 接口（claude-status 没做，可能是 Warp 没暴露）
- **VS Code**：考虑用 `code --reuse-window <folder>` 打开对应项目（不是精确 pane 但能定位到项目）
- ** Kitty / WezTerm**：这两个其实有自己的 remote control socket（kitty remote control、wezterm cli），但需要用户预启用 —— claude-status 没做，可作为 cc-view 的差异化点

### 6.2 要不要加 Compacting 状态（强烈建议加）

**理由**：
1. **用户感知强**：Compacting 通常发生在长会话，几秒到几十秒的"卡住"，没有专门状态会被误判成 Active 或 Idle，用户体验差
2. **实现成本不高**：cc-view 既然走"零侵入"路线（不装 plugin），需要从 JSONL transcript 推断状态。两个判据都不复杂：
   - 检测 JSONL 行 `"type":"system"` + `"subtype":"compact_boundary"` → Compacting
   - 检测任意行 `agentId` 字段以 `acompact-` 开头 → Compacting
3. **claude-status 的"抑制 replayed context"技巧要一起抄**：compaction 后 Claude Code 会写入 "This session is being continued..." 的 user 消息，必须像 claude-status 那样在 compacting 标志位下忽略 user/progress 消息直到下一个 assistant 响应，否则状态会乱跳

**与 cc-view 现有状态机的整合**：在 Active / Waiting / Idle 三态之上加 `Compacting`，优先级排序建议 `Waiting > Active > Compacting > Idle`（Waiting 仍然最高，因为权限弹窗比 compacting 更需要用户关注）。

### 6.3 Darwin notification 用不用（建议用，但要注意 sandbox）

**优点**：
- 比 `DispatchSource` 文件监听更即时（毫秒级 vs 秒级）
- 比 5s 轮询省 CPU
- 跨进程，不需要 poll file

**风险**：
- **Tauri 默认开 App Sandbox**，sandbox 下 `CFNotificationCenterGetDarwinNotifyCenter` 仍可用（这是少数在 sandbox 内可用的跨进程机制），但 `proc_pidinfo` / `sysctl KERN_PROCARGS2` / `osascript` 都**不能用**
- claude-status 直接禁用 sandbox（entitlements 只有 apple-events + app-groups）—— **这是 cc-view 必须做的决策**：要么像 claude-status 一样走 no-sandbox + Developer ID 签名 + 公证（不在 App Store 分发），要么放弃精确 focus（只做 app 级 activate，`open -a` 在 sandbox 内能用）

**结论**：cc-view 既然要做"零侵入监控 Claude Code 会话"，**几乎必然要 no-sandbox**（进程信息 + AppleScript 是核心能力）。Darwin notification 顺带就能用，FFI `notify_post` / `CFNotificationCenterAddObserver` 在 Rust 端需要自己绑定（`core-foundation` crate 提供 `CFString` / `CFNotificationCenter` 绑定）。

> **三重机制要不要全抄**：建议保留"Darwin + 5s 轮询"两层，**简化掉 DispatchSource**。理由：DispatchSource 是 plugin 写 `.cstatus` 场景下的 backup，cc-view 如果走零侵入（不装 plugin），状态来源本身就是 Tauri 后端自己 tail JSONL，已经在主进程内，不需要再跨文件通知。

### 6.4 其他可借鉴的设计

1. **`.cstatus` 文件格式 + 原子写**：如果 cc-view 也想做 widget / 多进程（Tauri 主进程 + 后台 daemon），用文件做 IPC 配合 atomic write（tempfile + rename）是稳健方案
2. **deadSessions 缓存**：避免每 5s 都重新尝试已知死的 session
3. **PID 回收防护**：`kill(pid, 0) == 0` 后再验证 `proc_pidpath` 含 `claude` / `node`，否则 PID 被回收给别的进程会误判
4. **daemon 架构（vs 每 hook 一个短命令）**：claude-status 从"14 个 Python hook"重写成"4 hook + 1 daemon tail JSONL"是重要教训——大部分状态变化 JSONL 里都有，hook 只用来 spawn daemon 和补 JSONL 缺失的信号（permission/elicitation/preCompact）
5. **URL scheme deep link**：`claude-status://session/<id>` 让 widget 点击跳回主 app 做 focus —— cc-view 的 Tauri 也能注册 URL scheme（`tauri.conf.json` 的 `app.windows` 或用 `tauri-plugin-deep-link`），widget 形态可参考
6. **NSAppleEventManager URL handler**（Swift）→ Tauri 的 deep-link 插件等价

---

## 7. 踩坑

### 7.1 tmux 进程树断层
tmux server 启动后会 reparent 到 pid 1（launchd），所以从 Claude 进程往上爬祖先链永远到不了外层终端 app。claude-status 的解法是**靠一系列会"漏"进 tmux session 的环境变量**反推：`LC_TERMINAL`（iTerm2 设）、`ITERM_SESSION_ID`、`GHOSTTY_RESOURCES_DIR`、`KITTY_PID`、`WEZTERM_PANE`、`ALACRITTY_SOCKET`。**cc-view 必须照抄这张表**，否则 tmux 下的 session 全部 fallback 到 `Terminal`。

### 7.2 AppleScript 注入防护
iTerm 的 `unique ID` 用字符串拼接进 AppleScript，必须用正则白名单 `^[A-Za-z0-9\-]+$` 校验，否则理论上恶意 session_id 能注入 AppleScript。Ghostty 的 cwd 拼接则用 `\` 和 `"` 转义。

### 7.3 `.cstatus` 必须原子写
menubar app 每 5s 读 `.cstatus`，如果 daemon 边写边被读，会读到半截 JSON 解析失败。claude-status 用 `tempfile::NamedTempFile::new_in(同目录)` + `persist(rename)` —— 必须同目录以保证 rename 是原子的（跨文件系统 rename 不是原子）。

### 7.4 `TMUX` 环境变量格式有歧义
格式 `/socket/path,pid,session`，但**socket 路径本身可能含逗号**（虽然罕见）。claude-status 用 suffix 解析（最后两段是 pid 和 session，前面全部是 socket path），不是简单 split 第一段：

```swift
let parts = tmuxEnv.components(separatedBy: ",")
if parts.count >= 3 {
    let suffixLen = parts[parts.count - 1].count + parts[parts.count - 2].count + 2
    tmuxSocket = String(tmuxEnv.dropLast(suffixLen))
}
```

### 7.5 Sparkle /公证 / App Group 三件套
不在 App Store 分发需要：Developer ID 签名 + 公证（`notarytool`）+ Sparkle appcast（EdDSA 签名）+ App Group provisioning profile（widget 和主 app 共享数据强制要求）。CI 流程需要 8 个 secret（证书 / provision profile / Apple ID / Sparkle 密钥对等）。

### 7.6 session resume 不要重复 spawn daemon
用户 resume 一个已有 session 时，Claude Code 会再触发一次 SessionStart。daemon 启动时把自己的 pid 写到 `.cpid`，SessionStart 检测到 `.cpid` 里的 daemon 还活着就只更新 `.cstatus` 不重新 spawn。

### 7.7 `acompact-` 前缀是非公开约定
长 compaction 启动的子 agent 用 `acompact-` 前缀的 agentId，这是 Claude Code 当前实现的行为但**未在文档中承诺**。未来 Claude Code 版本可能改变。cc-view 如果照抄这条判据需要做好兼容降级（至少 `compact_boundary` 系统消息 + PreCompact 信号这两条作为 backup）。

### 7.8 daemon 必须独立进程组
如果 daemon 和 hook runner 同进程组，hook runner 退出时会 SIGTERM 整组把 daemon 也杀了。`.process_group(0)` 让 daemon 成为新进程组的 leader，独立存活。

### 7.9 xattr `com.apple.provenance` 导致 Gatekeeper 杀进程
plugin 仓库的 CLAUDE.md 明确提到：编译完 Rust binary 拷贝到 `scripts/` 后**必须重新 codesign**（`codesign -fs -`），清掉下载/拷贝过程中沾上的 `com.apple.provenance` xattr，否则 Gatekeeper 会杀掉 hook 启动的进程。cc-view 如果分发独立 binary 也要注意。

### 7.10 进程树深度上限 8 层
祖先链最多爬 8 层，避免在极端嵌套场景（容器 / 多层 shell wrapper）下死循环或过长延迟。8 层覆盖了绝大多数场景：`shell → tmux pane → tmux server(parent 1) → ...` 实际命中通常在第 2-3 层。

---

## 8. 关键源文件链接

### 主 app（[gmr/claude-status](https://github.com/gmr/claude-status)，路径含空格）

| 文件 | 用途 |
|---|---|
| [`Claude Status/SessionDiscovery/TerminalFocuser.swift`](https://github.com/gmr/claude-status/blob/main/Claude%20Status/SessionDiscovery/TerminalFocuser.swift) | **focus 核心**：iTerm2/Ghostty AppleScript + tmux select-pane + NSRunningApplication.activate |
| [`Claude Status/SessionDiscovery/SessionDiscovery.swift`](https://github.com/gmr/claude-status/blob/main/Claude%20Status/SessionDiscovery/SessionDiscovery.swift) | **进程树分类**：proc_pidpath + sysctl KERN_PROCARGS2 + tmux env var fallback；PID 回收防护 |
| [`Claude Status/SessionDiscovery/SessionMonitor.swift`](https://github.com/gmr/claude-status/blob/main/Claude%20Status/SessionDiscovery/SessionMonitor.swift) | **三重接收**：CFNotificationCenter Darwin 通知 + DispatchSource + 5s timer |
| [`Claude Status/SessionDiscovery/StateResolver.swift`](https://github.com/gmr/claude-status/blob/main/Claude%20Status/SessionDiscovery/StateResolver.swift) | DispatchSource 文件监听实现；无 .cstatus 时从 JSONL mtime + 最后一行 fallback 推断状态 |
| [`Shared/ClaudeSession.swift`](https://github.com/gmr/claude-status/blob/main/Shared/ClaudeSession.swift) | `SessionState` 四态枚举（priority/sortOrder/emoji/color）；`SessionSource` 枚举；`ClaudeSession` 模型；deep link URL |
| [`Claude Status/AppMain.swift`](https://github.com/gmr/claude-status/blob/main/Claude%20Status/AppMain.swift) | 入口：单实例检查 + `app.setActivationPolicy(.accessory)`（无 Dock 图标）|
| [`Claude Status/AppDelegate.swift`](https://github.com/gmr/claude-status/blob/main/Claude%20Status/AppDelegate.swift) | NSStatusItem + NSPopover + NSAppleEventManager URL handler（widget deep link）+ Sparkle 初始化 |
| [`Claude Status/Views/SettingsView.swift`](https://github.com/gmr/claude-status/blob/main/Claude%20Status/Views/SettingsView.swift) | `SMAppService.mainApp.register/unregister` 实现 launch-at-login |
| [`Claude Status/Info.plist`](https://github.com/gmr/claude-status/blob/main/Claude%20Status/Info.plist) | `LSUIElement=true` / `NSAppleEventsUsageDescription` / Sparkle `SUFeedURL` / URL scheme `claude-status` |
| [`CLAUDE.md`](https://github.com/gmr/claude-status/blob/main/CLAUDE.md) | 项目自己的架构文档（含部署目标 / SPM 依赖 / CI/CD secrets / App Group 配置）|

### Plugin（[gmr/claude-status-plugin](https://github.com/gmr/claude-status-plugin)，Rust workspace）

| 文件 | 用途 |
|---|---|
| [`crates/session-status/src/main.rs`](https://github.com/gmr/claude-status-plugin/blob/main/crates/session-status/src/main.rs) | **plugin 核心**（~1700 行）：三模式（hook / daemon / signal）；JSONL 状态机；Compacting 检测；FFI `notify_post`；FFI `proc_pidinfo` 读 ppid |
| [`crates/set-session-name/src/main.rs`](https://github.com/gmr/claude-status-plugin/blob/main/crates/set-session-name/src/main.rs) | `/name-session` 实现：按 pid 找 `.cstatus`，原子更新 `session_name` 字段 |
| [`crates/jsonl-analyzer/src/main.rs`](https://github.com/gmr/claude-status-plugin/blob/main/crates/jsonl-analyzer/src/main.rs) | JSONL transcript schema 分析工具（开发期用，非运行时） |
| [`plugins/claude-status/hooks/hooks.json`](https://github.com/gmr/claude-status-plugin/blob/main/plugins/claude-status/hooks/hooks.json) | **5 个 hook 注册**（SessionStart / SessionEnd / PermissionRequest / Notification / PreCompact）|
| [`plugins/claude-status/.claude-plugin/plugin.json`](https://github.com/gmr/claude-status-plugin/blob/main/plugins/claude-status/.claude-plugin/plugin.json) | plugin 元数据（name=claude-status, version=2.0.5, BSD-3） |
| [`.claude-plugin/marketplace.json`](https://github.com/gmr/claude-status-plugin/blob/main/.claude-plugin/marketplace.json) | Claude Code marketplace 定义（`claude plugins install gmr/claude-status-plugin`）|
| [`CLAUDE.md`](https://github.com/gmr/claude-status-plugin/blob/main/CLAUDE.md) | plugin 架构文档（4 hook vs 老 12 hook 的演进、设计决策、`.cstatus` 格式） |
| [`README.md`](https://github.com/gmr/claude-status-plugin/blob/main/README.md) | plugin 用户向说明（安装方式 `claude plugins install gmr/claude-status-plugin`） |

### 安装

- 主 app：[Releases](https://github.com/gmr/claude-status/releases)（`.zip` / `.pkg`，Developer ID 签名 + 公证）
- Plugin：`claude plugins install gmr/claude-status-plugin`（通过 Claude Code marketplace）
