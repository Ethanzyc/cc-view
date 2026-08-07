# Token 统计与详情视图 设计

日期：2026-08-07
状态：待实现

## 背景与目标

cc-view 当前从 Claude Code 的 `~/.claude/projects/<编码cwd>/<sessionId>.jsonl` 只读取三样东西：会话标题（custom-title / ai-title）、末尾 pending tool_use（权限检测）、compact_boundary（压缩检测）。每条 assistant 消息自带的完整 `usage` token 数据**完全没被解析**。

本设计新增两件事：
- **A · 列表行累计 token**：overlay 会话列表每行显示该会话累计的输入/输出 token 量
- **B · 详情视图**：点"详情"进入某会话的 token 消耗明细（汇总 + 按回合）

附带优化：
- **C · actions 按钮图标化**：把当前文字按钮（搁置/归档/复制）+ 新增的详情按钮统一改成 Lucide 图标

## 明确不做：token 速度

经全面调研，JSONL 落盘数据中**没有任何计时字段**（`duration` / `elapsed` / `latency` / `streaming` / `first_token` 等全局搜索零命中），`~/.claude/` 下也无性能日志。Claude Code TUI 显示的实时 tokens/sec 是流式接收 SSE 时自行掐表、不落盘。

唯一能估算的是用 user→assistant 的 `timestamp` 差除以 output_tokens，但该值含排队 + thinking + tool 调用循环，严重偏低且抖动，语义不诚实。**经确认不做。**

## 数据源分析

### 能拿到什么

每条 `type == "assistant"` 行的 `message`：

| 字段 | 含义 |
|------|------|
| `usage.input_tokens` | 输入 token |
| `usage.output_tokens` | 输出 token |
| `usage.cache_read_input_tokens` | 缓存命中 token |
| `usage.cache_creation_input_tokens` | 缓存写入 token |
| `usage.server_tool_use.web_search_requests` | web 搜索次数 |
| `usage.server_tool_use.web_fetch_requests` | web 抓取次数 |
| `model` | 模型名（如 `glm-5.2`） |
| `content[]` | 含 `text` / `thinking` / `tool_use` 三种 block |

每条 `type == "user"` 行的 `message.content`：
- **string**：真实用户输入（开新回合）
- **array**：含 `text` block（真实输入，开新回合）或 `tool_result` block（工具返回，**不开新回合**）

### 性能结论（重要修正）

现有 `read_session_title` 已经对**每个活会话**全文遍历 JSONL（拿标题）。A 的 token 累加**搭这趟车**——在同一遍遍历里顺带累加 usage，零额外 IO、无需独立缓存。

`stats-cache.json` 是全局统计（按天/按模型聚合），**无 per-session 数据**，不可复用。

## 回合定义

"回合"（turn）= 一次真实用户输入 + 后续到下一次真实用户输入之前的所有 assistant / tool_result 行。

分组规则（遍历状态机）：
1. 遇到 `user` 且 content 是 **string** 或 array 含 **text** block → 结束当前回合、开启新回合；`prompt` 预览取该 text 前 ~40 字，`ts` 取该行 timestamp
2. 遇到 `user` 且 content 全是 **tool_result** → **不开新回合**（属当前回合的工具返回）
3. 遇到 `assistant` → 累加其 `usage` 到汇总 + 当前回合；统计 content 中 `tool_use` block 数

> 不区分规则 1/2 会把每个 tool_result 误算成一个用户回合，回合数虚高、预览错乱。

## A · 列表行累计 token

### 模型变更

`models.rs` 的 `Session` 加两个字段（`#[serde(default)]` 兼容旧缓存/前端）：

```rust
#[serde(default)]
pub tokens_in: u64,
#[serde(default)]
pub tokens_out: u64,
```

`types.ts` 的 `Session` 同步加 `tokensIn: number` / `tokensOut: number`。

### 后端

把现有 `read_session_title`（全文读 + 遍历取标题）升级为一次遍历同时产出标题 + 累计 token：

- 新增 `scan_session_jsonl(path) -> SessionScan { title: Option<String>, tokens_in: u64, tokens_out: u64 }`
- `parse_session_title` 的逐行逻辑扩展：解析每行时若 `type == "assistant"` 且有 `message.usage`，累加 input/output tokens
- `collect_sessions` 用 `scan_session_jsonl` 替换 `read_session_title`，把 tokens 写入 `Session`
- `parse_session_title` 保留为纯标题解析（测试已依赖），`scan_session_jsonl` 内部可复用同一遍历

`JsonlRow` / `JsonlMessage` 反序列化 struct 补 `usage`（可选字段，serde 自动忽略未声明字段，成本极低）。

### 前端展示

- 位置：`ago` **左边**（同一行右端数据列）
- 格式：`12.3k↑ 4.1k↓`，等宽数字（`font-variant-numeric: tabular-nums`），tertiary 色
- 格式化函数：`<1000 → 原数`，`≥1000 → 一位小数 + k`
- **不含 cache**（cache 命中量级远大于真实 I/O，会盖过；放详情）

数值为 0（会话无 assistant 消息，如纯 shell）时不显示该列，避免噪音。

## B · 详情视图

### 入口与导航

- PanelView **内部**加 detail 子状态（`selectedDetail: SessionDetail | null`），**不动**全局 mode、不动窗口尺寸（560×420 够用）
- actions 加"详情"按钮（图标见 C）→ `openDetail(id)`：invoke 后端 → 置 `selectedDetail`
- 模板：`v-if="selectedDetail"` 渲染详情，`v-else` 渲染原列表
- 详情顶栏 ← 返回按钮 → 清空 `selectedDetail` 回列表

抽独立组件 `DetailPanel.vue`（PanelView 已 800+ 行，不宜继续膨胀）。PanelView 负责 selectedDetail 状态管理 + 传递 props。

### 数据模型

新增（`models.rs` + `types.ts`，camelCase 序列化）：

```rust
pub struct SessionDetail {
    pub session_id: String,
    // 汇总
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub model: String,          // 最近一条 assistant 的 model
    pub turn_count: u32,
    pub tool_calls: u32,        // tool_use block 总数
    pub web_searches: u32,
    pub web_fetches: u32,
    pub turns: Vec<TurnStat>,
}

pub struct TurnStat {
    pub idx: u32,               // 从 1 起
    pub prompt: String,         // 用户输入前 ~40 字（截断）
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub tool_calls: u32,
    pub ts: i64,                // 回合起始 timestamp → epoch ms
}
```

### 后端

- 新 command `get_session_detail(id: String) -> Option<SessionDetail>`
  - 从现有 sessions cache（`Mutex<Vec<Session>>`）按 id 查 cwd → 定位 JSONL 文件
  - 调 `scan_jsonl_detail(path) -> SessionDetail`：一遍遍历，按上文回合状态机产出汇总 + turns
  - on-demand 调用（点详情才触发），**不需缓存**
- timestamp 解析：ISO 8601 `2026-08-06T08:39:37.573Z` → epoch ms（用 `chrono` 或手写解析；项目已依赖的 crate 优先）

### 前端展示（DetailPanel.vue）

布局（560×420 内竖向滚动）：

```
← 返回                                       <会话名>
─────────────────────────────────────────────
汇总
  输入 45.2k    输出 8.1k    缓存命中 120.3k
  模型 glm-5.2   回合 12   工具调用 61   搜索 3
─────────────────────────────────────────────
按回合
  #1  帮我优化 token 显示…     1.2k↑ 340↓  🔧3   2h
  #2  改一下按钮成图标…        3.4k↑ 580↓  🔧5   1h
  ...
```

- 汇总区：等宽对齐的键值网格
- 回合列表：每行 idx + prompt（截断省略）+ in/out + 工具数 + ago；prompt 为空（纯 tool_result 起手等异常）显示 `—`
- 样式复用现有 CSS token（`--color-*` / `--fs-*` / tabular-nums），与列表视觉一致

## C · actions 按钮图标化

当前 actions 文字按钮（搁置/恢复、归档、复制）+ 新增详情 = 4 个。全改 13px Lucide stroke 图标，与现有 pin / collapse 按钮风格统一（`stroke-width=2`、`currentColor`、hover `--color-fg` + `--color-hover`）。

| 动作 | 图标（Lucide） | 备注 |
|------|------|------|
| 详情 | `chart-no-axes-column`（柱状图） | 表达"统计明细"，区别于 info 的泛信息 |
| 搁置 | `moon` | 睡眠语义 |
| 恢复 | `sun` | 唤醒语义（与 moon 对称） |
| 归档 | `archive` | |
| 取消归档 | `archive-restore` | |
| 复制 | `copy` | |
| 已复制 | `check` | 复制成功态（已有逻辑，改图标） |

每个按钮保留 `title` / `aria-label` 文字提示，保证可发现性与无障碍。按钮尺寸沿用现有 `.act-btn`（padding 不变，只是内容从文字换 SVG）。

## 文件变更清单

**后端（src-tauri/src/）**
- `models.rs`：Session 加 `tokens_in` / `tokens_out`；新增 `SessionDetail` / `TurnStat`
- `collector.rs`：`scan_session_jsonl`（title + tokens 合并遍历）、`scan_jsonl_detail`（详情遍历）；`collect_sessions` 接入 tokens
- `lib.rs`：注册 `get_session_detail` command；sessions cache 查 cwd 辅助

**前端（src/）**
- `types.ts`：Session 加 `tokensIn` / `tokensOut`；新增 `SessionDetail` / `TurnStat` 接口
- `utils/session.ts`：token 格式化函数（`fmtTok`）
- `components/PanelView.vue`：行内 token 列、selectedDetail 状态、openDetail、详情按钮图标化、其余 actions 图标化
- `components/DetailPanel.vue`：新增详情组件

**测试**
- `collector.rs` 单测：`scan_session_jsonl` token 累加；`scan_jsonl_detail` 回合分组（**重点测 tool_result 不开新回合**）、汇总字段、timestamp 解析
- 新增 fixture：含多回合 + tool_result + 多模型的 jsonl 片段

## 风险与取舍

- **大文件遍历**：7MB JSONL 全文遍历在 A 路径已存在（取标题），合并 token 不增加 IO；B 路径 on-demand 单次遍历可接受。若未来活会话很多导致 3s 轮询吃力，再加 size 缓存（本设计刻意先不做，避免过度设计）。
- **回合 prompt 预览**：取 user content 首个 text，截断 40 字；含命令注入标记（如 `<command-name>`）的 string 原样截断显示，不做清洗（与 Claude Code 自身一致）。
- **模型字段**：只取最近一条 assistant 的 model；一个会话中途换模型的情况不展示历史分布（YAGNI）。
