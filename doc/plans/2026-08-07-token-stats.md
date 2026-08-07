# Token 统计与详情视图 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 cc-view 加会话级 token 统计——列表行显示累计 input/output token，点详情看汇总 + 按回合明细；顺带把 actions 文字按钮图标化。

**Architecture:** 后端 Rust 在现有"取标题"的全文遍历里顺带累加 token（零额外 IO），新增 on-demand `get_session_detail` command 做回合分组详情扫描；前端 PanelView 内部加 detail 子状态切换到新 `DetailPanel.vue`，不动全局 mode/窗口尺寸。

**Tech Stack:** Rust + Tauri 2 + serde（后端）；Vue 3 + TS（前端，无单测框架，前端任务用 `npm run tauri dev` verify）。

## Global Constraints

- 后端 struct 序列化统一 `#[serde(rename_all = "camelCase")]`，前端 TS 直接用 camelCase。
- 错误处理 fail fast：单文件/单会话解析失败隔离（skip + eprintln），不拖垮整体。
- 图标统一 13px、`viewBox="0 0 24 24"`、`stroke-width="2"`、`stroke="currentColor"`、`fill="none"`（与现有 pin/collapse/expand 按钮一致）。
- CSS 复用现有 token（`--color-*` / `--fs-*` / `--fw-*` / `--font-utility` / `--motion-*`），数字列加 `font-variant-numeric: tabular-nums`。
- 注释中文，代码英文。
- 测试用 `cargo test`（后端），fixture 放 `src-tauri/tests/fixtures/`。

---

## File Structure

**后端**
- `src-tauri/src/models.rs` — `Session` 加 `tokens_in/tokens_out`；新增 `SessionDetail` / `TurnStat`
- `src-tauri/src/collector.rs` — `scan_session_jsonl`（title+token 合并遍历）、`scan_session_detail`（回合分组详情）；`collect_sessions` 接入
- `src-tauri/src/lib.rs` — `hash_sessions` 加 token 字段；新增 `get_session_detail` command 并注册

**前端**
- `src/types.ts` — `Session` 加 `tokensIn/tokensOut`；新增 `SessionDetail` / `TurnStat`
- `src/utils/session.ts` — `fmtTok` 格式化函数
- `src/components/PanelView.vue` — 行内 token 列、selectedDetail 状态、openDetail、详情按钮 + actions 图标化
- `src/components/DetailPanel.vue` — **新增**详情组件

---

### Task 1: 后端 — Session 加 token 字段 + 累加扫描 + hash 更新

**Files:**
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/collector.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/collector.rs`（内联文本单测，无需 fixture）

**Interfaces:**
- Produces: `Session.tokens_in: u64` / `Session.tokens_out: u64`（`#[serde(default)]`，camelCase → `tokensIn/tokensOut`）；`pub fn scan_session_jsonl(session_id: &str, cwd: &str) -> Option<SessionScan>`；`pub struct SessionScan { title: Option<String>, tokens_in: u64, tokens_out: u64 }`

- [ ] **Step 1: models.rs — Session 加两个字段**

在 `src-tauri/src/models.rs` 的 `Session` struct，`snoozed` 字段后追加（保持 `#[serde(default)]` 兼容旧缓存）：

```rust
    /// 累计 token（collect 时 scan JSONL 填充，roster/agents 无 JSONL 时为 0）。
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
```

- [ ] **Step 2: 补全所有 Session 构造点**

每处 `Session { ... }` 字面量加 `tokens_in: 0, tokens_out: 0,`。共 5 处：

1. `collector.rs` `parse_session_file` 的 `Ok(Session { ... })`
2. `collector.rs` `parse_roster` 闭包里的 `Session { ... }`
3. `collector.rs` `parse_agents` 闭包里的 `Session { ... }`
4. `collector.rs` 测试 `agents_fleet_overrides_roster_in_reducer` 的 `roster_session`
5. `models.rs` 测试 `session_serializes_to_camel_case` 的 `Session { ... }`

- [ ] **Step 3: collector.rs — 新增 scan struct 与纯函数**

在 `collector.rs` 文件末尾的 `mod tests` **之前**插入：

```rust
// ---- 全文 scan：标题 + 累计 token 一遍遍历（替代 read_session_title 单一职责）----
// 零额外 IO：取标题本来就要读全文，顺带累加 assistant usage。
#[derive(serde::Deserialize)]
struct ScanRow {
    #[serde(rename = "type")]
    row_type: Option<String>,
    message: Option<ScanMessage>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
}
#[derive(serde::Deserialize)]
struct ScanMessage {
    #[serde(default)]
    usage: Option<ScanUsage>,
}
#[derive(serde::Deserialize)]
struct ScanUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

/// 一遍遍历的产出：标题（custom ?? ai）+ 累计 input/output token。
pub struct SessionScan {
    pub title: Option<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

/// 纯函数：从 JSONL 全文文本一遍遍历，取标题 + 累计 token。便于单测。
pub fn scan_session_jsonl_from_text(text: &str) -> SessionScan {
    let mut custom: Option<String> = None;
    let mut ai: Option<String> = None;
    let mut tokens_in: u64 = 0;
    let mut tokens_out: u64 = 0;
    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<ScanRow>(line) else {
            continue;
        };
        match row.row_type.as_deref() {
            Some("custom-title") => custom = row.custom_title,
            Some("ai-title") => ai = row.ai_title,
            Some("assistant") => {
                if let Some(u) = row.message.as_ref().and_then(|m| m.usage.as_ref()) {
                    tokens_in += u.input_tokens;
                    tokens_out += u.output_tokens;
                }
            }
            _ => {}
        }
    }
    SessionScan { title: custom.or(ai), tokens_in, tokens_out }
}

/// 读 ~/.claude/projects/<encoded-cwd>/<session-id>.jsonl 全文，一遍出标题 + token。
pub fn scan_session_jsonl(session_id: &str, cwd: &str) -> Option<SessionScan> {
    let home = dirs::home_dir()?;
    let encoded = cwd.replace('/', "-");
    let path = home
        .join(".claude/projects")
        .join(&encoded)
        .join(format!("{}.jsonl", session_id));
    let text = std::fs::read_to_string(&path).ok()?;
    Some(scan_session_jsonl_from_text(&text))
}

/// 对单个活会话填充累计 token（不碰 name）。roster/agents 共用。
fn fill_tokens(s: &mut Session) {
    if let Some(scan) = scan_session_jsonl(&s.id, &s.cwd) {
        s.tokens_in = scan.tokens_in;
        s.tokens_out = scan.tokens_out;
    }
}
```

- [ ] **Step 4: collect_sessions — interactive 用 scan 替换 read_session_title**

在 `collect_sessions` 中，把 interactive alive 块里的标题读取替换为 scan（同时填 title + tokens）。原代码：

```rust
                    if let Some(title) = read_session_title(&s.id, &s.cwd) {
                        s.name = title;
                    }
```

改为：

```rust
                    if let Some(scan) = scan_session_jsonl(&s.id, &s.cwd) {
                        if let Some(t) = scan.title {
                            s.name = t;
                        }
                        s.tokens_in = scan.tokens_in;
                        s.tokens_out = scan.tokens_out;
                    }
```

- [ ] **Step 5: collect_sessions — roster 与 agents 填 token**

roster 循环里，`if w.alive { ... }` 块内 `w.focus_hint.host = ...` 之前加一行：

```rust
        fill_tokens(&mut w);
```

agents 合并段，把

```rust
    out.extend(
        read_agents()
            .into_iter()
            .filter(|s| s.source == crate::models::Source::Fleet),
    );
```

改为：

```rust
    let mut agents: Vec<Session> = read_agents()
        .into_iter()
        .filter(|s| s.source == crate::models::Source::Fleet)
        .collect();
    for s in agents.iter_mut().filter(|s| s.alive) {
        fill_tokens(s);
    }
    out.extend(agents);
```

- [ ] **Step 6: lib.rs — hash_sessions 加 token 字段**

`hash_sessions` 加两行（否则 token 增长不触发 emit，列表数字不刷新）：

```rust
fn hash_sessions(s: &[models::Session]) -> u64 {
    let mut h = DefaultHasher::new();
    for x in s {
        x.id.hash(&mut h);
        format!("{:?}", x.status).hash(&mut h);
        x.alive.hash(&mut h);
        x.snoozed.hash(&mut h);
        x.tokens_in.hash(&mut h);
        x.tokens_out.hash(&mut h);
    }
    h.finish()
}
```

- [ ] **Step 7: 写失败测试**

在 `collector.rs` 的 `mod tests` 内追加：

```rust
    #[test]
    fn scan_totals_accumulate_assistant_usage() {
        let text = "\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input_tokens\":100,\"output_tokens\":50}}}
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input_tokens\":200,\"output_tokens\":80}}}";
        let scan = super::scan_session_jsonl_from_text(text);
        assert_eq!(scan.tokens_in, 300);
        assert_eq!(scan.tokens_out, 130);
        assert_eq!(scan.title, None);
    }

    #[test]
    fn scan_picks_custom_title_over_ai_with_tokens() {
        let text = "\
{\"type\":\"ai-title\",\"aiTitle\":\"AI标题\"}
{\"type\":\"custom-title\",\"customTitle\":\"用户改名\"}
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}";
        let scan = super::scan_session_jsonl_from_text(text);
        assert_eq!(scan.title.as_deref(), Some("用户改名"));
        assert_eq!(scan.tokens_in, 10);
        assert_eq!(scan.tokens_out, 5);
    }
```

- [ ] **Step 8: 跑测试确认通过**

Run: `cd src-tauri && cargo test scan_ -- --nocapture`
Expected: 2 passed（含 `scan_totals_accumulate_assistant_usage`、`scan_picks_custom_title_over_ai_with_tokens`）。同时 `cargo test`（全量）应仍全绿（5 处构造点已补全）。

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/collector.rs src-tauri/src/lib.rs
git commit -m "feat: 会话累计 token 后端扫描（搭取标题遍历的车）"
```

---

### Task 2: 前端 — 列表行显示累计 token

**Files:**
- Modify: `src/types.ts`
- Modify: `src/utils/session.ts`
- Modify: `src/components/PanelView.vue`
- Verify: `npm run tauri dev`，看 overlay 行尾出现 token

**Interfaces:**
- Consumes: `Session.tokensIn` / `Session.tokensOut`（来自 Task 1 后端 emit）

- [ ] **Step 1: types.ts — Session 加字段**

`src/types.ts` 的 `Session` interface，`snoozed` 后加：

```ts
  // 累计 token（后端 scan JSONL 填充；纯 shell 会话为 0）
  tokensIn: number;
  tokensOut: number;
```

- [ ] **Step 2: utils/session.ts — fmtTok**

`src/utils/session.ts` 末尾加：

```ts
// token 量格式化：<1000 原数，≥1000 一位小数 + k（1000→1k，12345→12.3k）。
export function fmtTok(n: number): string {
  if (n < 1000) return String(n);
  return (n / 1000).toFixed(1).replace(/\.0$/, '') + 'k';
}
```

- [ ] **Step 3: PanelView.vue — import fmtTok**

`PanelView.vue` 顶部 import 行（第 14 行）追加 `fmtTok`：

```ts
import { STATUS_ZH, statusRank, projShort, agoF, isFresh, isStaleInput, hlParts, fmtTok } from '../utils/session';
```

- [ ] **Step 4: PanelView.vue — 搜索态行加 token 列**

搜索态 `<li class="row">` 里，`<span class="ago" ...>` **之前**插入：

```html
          <span v-if="s.tokensIn || s.tokensOut" class="tok">
            {{ fmtTok(s.tokensIn) }}<span class="arr">↑</span>{{ fmtTok(s.tokensOut) }}<span class="arr">↓</span>
          </span>
```

- [ ] **Step 5: PanelView.vue — 非搜索态行加 token 列**

非搜索态 `<li class="row">`（`v-for="s in rows"`）里，`<span class="ago" ...>` **之前**插入同样一段（与 Step 4 完全一致）：

```html
              <span v-if="s.tokensIn || s.tokensOut" class="tok">
                {{ fmtTok(s.tokensIn) }}<span class="arr">↑</span>{{ fmtTok(s.tokensOut) }}<span class="arr">↓</span>
              </span>
```

- [ ] **Step 6: PanelView.vue — CSS**

`<style scoped>` 内，`.ago` 规则**之前**加：

```css
/* token 列：ago 左边，等宽数据列，箭头淡一点 */
.tok {
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-utility);
  color: var(--color-tertiary);
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  font-variant-numeric: tabular-nums;
}
.tok .arr {
  opacity: 0.6;
  margin: 0 1px;
}
```

- [ ] **Step 7: Verify**

Run: `npm run tauri dev`（首次会编译 Rust，稍慢）
Expected: 呼出 overlay（⌥Space），每个有对话的会话行尾、ago 左边显示如 `12.3k↑4.1k↓`；纯 shell 会话不显示该列。

- [ ] **Step 8: Commit**

```bash
git add src/types.ts src/utils/session.ts src/components/PanelView.vue
git commit -m "feat: 列表行显示累计 token（input/output，ago 左边）"
```

---

### Task 3: 后端 — get_session_detail command + 回合分组扫描

**Files:**
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/collector.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/collector.rs`（内联文本单测）

**Interfaces:**
- Produces: `models::SessionDetail`、`models::TurnStat`；`pub fn scan_session_detail(session_id: &str, cwd: &str) -> Option<SessionDetail>`；`pub fn scan_detail_from_text(text: &str) -> SessionDetail`；Tauri command `get_session_detail(id: String) -> Option<SessionDetail>`
- Consumes: `Session.cwd`（从 sessions cache 查 id 定位 JSONL）

- [ ] **Step 1: models.rs — SessionDetail / TurnStat**

`src-tauri/src/models.rs` 文件末尾（`mod tests` 之前）加：

```rust
/// 单个会话的 token 消耗详情（on-demand：点详情才扫描）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session_id: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub model: String, // 最近一条 assistant 的 model
    pub turn_count: u32,
    pub tool_calls: u32,
    pub web_searches: u32,
    pub web_fetches: u32,
    pub turns: Vec<TurnStat>,
}

/// 按回合的消耗明细。ts 为原始 ISO 8601 字符串，前端 Date.parse 转 ms（省后端解析）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStat {
    pub idx: u32,
    pub prompt: String, // 用户输入前 40 字
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub tool_calls: u32,
    pub ts: String,
}
```

- [ ] **Step 2: collector.rs — detail scan struct + 辅助函数**

在 Task 1 新增的 `fill_tokens` **之后**、`mod tests` 之前插入：

```rust
// ---- 详情扫描：回合分组 + 汇总（on-demand，无缓存）----
#[derive(serde::Deserialize)]
struct DetailRow {
    #[serde(rename = "type")]
    row_type: Option<String>,
    message: Option<DetailMessage>,
    timestamp: Option<String>,
}
#[derive(serde::Deserialize)]
struct DetailMessage {
    #[serde(default)]
    model: Option<String>,
    // content 可能是 string 或 array，用 Value 统一处理
    #[serde(default)]
    content: Option<serde_json::Value>,
    #[serde(default)]
    usage: Option<DetailUsage>,
}
#[derive(serde::Deserialize)]
struct DetailUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    server_tool_use: Option<DetailServerToolUse>,
}
#[derive(serde::Deserialize)]
struct DetailServerToolUse {
    #[serde(default)]
    web_search_requests: u64,
    #[serde(default)]
    web_fetch_requests: u64,
}

/// 从 user message content 提取真实用户输入文本。
/// string → 整段；array → 取首个 type=="text" 的 text；纯 tool_result → None（不开回合）。
fn extract_user_text(content: &serde_json::Value) -> Option<String> {
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        for item in arr {
            if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

/// 统计 assistant content 中 tool_use block 数。
fn count_tool_use(content: &serde_json::Value) -> u32 {
    content
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|item| item.get("type").and_then(|v| v.as_str()) == Some("tool_use"))
                .count() as u32
        })
        .unwrap_or(0)
}

/// 纯函数：一遍遍历 JSONL 文本，按回合分组 + 累计汇总。session_id 留空，由调用方填。
/// 回合定义：真实用户输入（string 或含 text block）开新回合；tool_result 不开回合。
pub fn scan_detail_from_text(text: &str) -> crate::models::SessionDetail {
    use crate::models::{SessionDetail, TurnStat};
    let mut tokens_in = 0u64;
    let mut tokens_out = 0u64;
    let mut cache_read = 0u64;
    let mut cache_creation = 0u64;
    let mut model = String::new();
    let mut tool_calls = 0u32;
    let mut web_searches = 0u64;
    let mut web_fetches = 0u64;
    let mut turns: Vec<TurnStat> = Vec::new();
    let mut cur: Option<TurnStat> = None;
    let mut turn_idx = 0u32;

    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<DetailRow>(line) else {
            continue;
        };
        match row.row_type.as_deref() {
            Some("user") => {
                let Some(msg) = row.message.as_ref() else { continue };
                let Some(content) = msg.content.as_ref() else { continue };
                if let Some(prompt) = extract_user_text(content) {
                    // 真实输入 → 结束当前回合、开新回合
                    if let Some(t) = cur.take() {
                        turns.push(t);
                    }
                    turn_idx += 1;
                    cur = Some(TurnStat {
                        idx: turn_idx,
                        prompt: prompt.chars().take(40).collect(),
                        tokens_in: 0,
                        tokens_out: 0,
                        tool_calls: 0,
                        ts: row.timestamp.clone().unwrap_or_default(),
                    });
                }
                // tool_result：不开回合，忽略
            }
            Some("assistant") => {
                let Some(msg) = row.message.as_ref() else { continue };
                if let Some(u) = msg.usage.as_ref() {
                    tokens_in += u.input_tokens;
                    tokens_out += u.output_tokens;
                    cache_read += u.cache_read_input_tokens;
                    cache_creation += u.cache_creation_input_tokens;
                    if let Some(stu) = u.server_tool_use.as_ref() {
                        web_searches += stu.web_search_requests;
                        web_fetches += stu.web_fetch_requests;
                    }
                }
                if let Some(m) = msg.model.as_ref().filter(|m| !m.is_empty()) {
                    model = m.clone();
                }
                let tc = msg.content.as_ref().map(count_tool_use).unwrap_or(0);
                tool_calls += tc;
                if let Some(t) = cur.as_mut() {
                    if let Some(u) = msg.usage.as_ref() {
                        t.tokens_in += u.input_tokens;
                        t.tokens_out += u.output_tokens;
                    }
                    t.tool_calls += tc;
                }
            }
            _ => {}
        }
    }
    if let Some(t) = cur.take() {
        turns.push(t);
    }
    let turn_count = turns.len() as u32;
    SessionDetail {
        session_id: String::new(),
        tokens_in,
        tokens_out,
        cache_read,
        cache_creation,
        model,
        turn_count,
        tool_calls,
        web_searches: web_searches as u32,
        web_fetches: web_fetches as u32,
        turns,
    }
}

/// 读 JSONL 全文做详情扫描。
pub fn scan_session_detail(session_id: &str, cwd: &str) -> Option<crate::models::SessionDetail> {
    let home = dirs::home_dir()?;
    let encoded = cwd.replace('/', "-");
    let path = home
        .join(".claude/projects")
        .join(&encoded)
        .join(format!("{}.jsonl", session_id));
    let text = std::fs::read_to_string(&path).ok()?;
    let mut d = scan_detail_from_text(&text);
    d.session_id = session_id.to_string();
    Some(d)
}
```

- [ ] **Step 3: collector.rs — 写失败测试**

`mod tests` 内追加（**重点测 tool_result 不开新回合**）：

```rust
    #[test]
    fn scan_detail_groups_turns_and_accumulates() {
        let text = "\
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"第一问\"}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:00:05.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"glm-5.2\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Read\"}],\"usage\":{\"input_tokens\":100,\"output_tokens\":40}}}
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:00:06.000Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"x\",\"content\":\"ok\"}]}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"glm-5.2\",\"content\":[{\"type\":\"text\",\"text\":\"答1\"}],\"usage\":{\"input_tokens\":50,\"output_tokens\":20}}}
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:01:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"第二问\"}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:01:05.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"glm-5.2\",\"content\":[{\"type\":\"text\",\"text\":\"答2\"}],\"usage\":{\"input_tokens\":30,\"output_tokens\":10}}}";
        let d = super::scan_detail_from_text(text);
        assert_eq!(d.turn_count, 2); // tool_result 不开回合
        assert_eq!(d.tokens_in, 180); // 100+50+30
        assert_eq!(d.tokens_out, 70); // 40+20+10
        assert_eq!(d.tool_calls, 1);
        assert_eq!(d.model, "glm-5.2");
        // 回合1：含 tool_use assistant + text assistant，tool_result 归入回合1
        assert_eq!(d.turns[0].idx, 1);
        assert_eq!(d.turns[0].prompt, "第一问");
        assert_eq!(d.turns[0].tokens_in, 150); // 100+50
        assert_eq!(d.turns[0].tokens_out, 60); // 40+20
        assert_eq!(d.turns[0].tool_calls, 1);
        assert_eq!(d.turns[0].ts, "2026-08-07T01:00:00.000Z");
        assert_eq!(d.turns[1].idx, 2);
        assert_eq!(d.turns[1].prompt, "第二问");
    }

    #[test]
    fn scan_detail_empty_text_returns_zero() {
        let d = super::scan_detail_from_text("");
        assert_eq!(d.turn_count, 0);
        assert_eq!(d.tokens_in, 0);
        assert!(d.turns.is_empty());
    }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test scan_detail -- --nocapture`
Expected: 2 passed。

- [ ] **Step 5: lib.rs — get_session_detail command**

`src-tauri/src/lib.rs`，在 `get_sessions` command **之后**加：

```rust
/// 返回某会话的 token 消耗详情（汇总 + 按回合）。on-demand：点详情才扫描。
/// 从 sessions cache 按 id 查 cwd 定位 JSONL；找不到 id 或文件缺失返回 None。
#[tauri::command]
fn get_session_detail(
    id: String,
    cache: tauri::State<'_, Mutex<Vec<models::Session>>>,
) -> Option<models::SessionDetail> {
    let cwd = cache
        .lock()
        .ok()
        .and_then(|s| s.iter().find(|s| s.id == id).map(|s| s.cwd.clone()))?;
    collector::scan_session_detail(&id, &cwd)
}
```

- [ ] **Step 6: lib.rs — 注册 command**

在 `tauri::generate_handler![...]` 列表里（`get_sessions,` 之后）加 `get_session_detail,`：

```rust
            get_sessions,
            get_session_detail,
            get_overlay_pinned,
```

- [ ] **Step 7: 全量编译 + 测试**

Run: `cd src-tauri && cargo test`
Expected: 全绿（含新增 2 个 scan_detail 测试 + 既有测试）。

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/collector.rs src-tauri/src/lib.rs
git commit -m "feat: get_session_detail command（回合分组 token 详情扫描）"
```

---

### Task 4: 前端 — DetailPanel 组件 + 详情入口

**Files:**
- Modify: `src/types.ts`
- Create: `src/components/DetailPanel.vue`
- Modify: `src/components/PanelView.vue`
- Verify: `npm run tauri dev`，点详情进/出详情视图

**Interfaces:**
- Consumes: `get_session_detail` command（Task 3）；`Session`（取 name/project）

- [ ] **Step 1: types.ts — SessionDetail / TurnStat**

`src/types.ts` 末尾加：

```ts
// 按回合的消耗明细（与后端 TurnStat camelCase 对齐）
export interface TurnStat {
  idx: number;
  prompt: string;
  tokensIn: number;
  tokensOut: number;
  toolCalls: number;
  ts: string; // ISO 8601 原始字符串，前端 Date.parse
}

// get_session_detail 返回的完整详情
export interface SessionDetail {
  sessionId: string;
  tokensIn: number;
  tokensOut: number;
  cacheRead: number;
  cacheCreation: number;
  model: string;
  turnCount: number;
  toolCalls: number;
  webSearches: number;
  webFetches: number;
  turns: TurnStat[];
}
```

- [ ] **Step 2: 新建 DetailPanel.vue**

创建 `src/components/DetailPanel.vue`：

```vue
<script setup lang="ts">
// 会话 token 详情：汇总 + 按回合。PanelView 内部子状态切入，不动全局 mode/窗口。
import type { SessionDetail } from '../types';
import { fmtTok, agoF } from '../utils/session';

defineProps<{ detail: SessionDetail; name: string }>();
defineEmits<{ back: [] }>();
</script>

<template>
  <div class="detail">
    <div class="detail-bar" data-tauri-drag-region="deep">
      <button
        class="back-btn"
        title="返回列表"
        aria-label="返回列表"
        data-tauri-drag-region="false"
        @click="$emit('back')"
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M19 12H5" /><path d="M12 19l-7-7 7-7" />
        </svg>
      </button>
      <span class="detail-title">{{ name }}</span>
    </div>
    <div class="divider" />
    <div class="detail-scroll">
      <div class="summary">
        <div class="sum-row">
          <span class="sum"><b>{{ fmtTok(detail.tokensIn) }}</b><i>输入</i></span>
          <span class="sum"><b>{{ fmtTok(detail.tokensOut) }}</b><i>输出</i></span>
          <span class="sum"><b>{{ fmtTok(detail.cacheRead) }}</b><i>缓存命中</i></span>
        </div>
        <div class="sum-row sub">
          <span>{{ detail.model || '—' }}</span>
          <span>{{ detail.turnCount }} 回合</span>
          <span>{{ detail.toolCalls }} 工具</span>
          <span v-if="detail.webSearches || detail.webFetches">{{ detail.webSearches }} 搜 / {{ detail.webFetches }} 抓</span>
        </div>
      </div>
      <div class="divider" />
      <div class="turns-head">按回合</div>
      <ul class="turns">
        <li v-for="t in detail.turns" :key="t.idx" class="turn">
          <span class="t-idx">#{{ t.idx }}</span>
          <span class="t-prompt">{{ t.prompt || '—' }}</span>
          <span class="t-tok">{{ fmtTok(t.tokensIn) }}<span class="arr">↑</span>{{ fmtTok(t.tokensOut) }}<span class="arr">↓</span></span>
          <span v-if="t.toolCalls" class="t-tools">🔧{{ t.toolCalls }}</span>
          <span class="t-ago">{{ t.ts ? agoF(new Date(t.ts).getTime()) : '' }}</span>
        </li>
      </ul>
    </div>
  </div>
</template>

<style scoped>
.detail {
  background: var(--color-bg-overlay);
  color: var(--color-fg);
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  border-radius: var(--radius-overlay);
  overflow: hidden;
}
.detail-bar {
  display: flex;
  align-items: center;
  gap: var(--gap);
  padding: var(--pad-y) var(--pad-x);
}
.back-btn {
  background: none;
  border: none;
  color: var(--color-tertiary);
  cursor: pointer;
  padding: 2px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  transition: color var(--motion-duration) var(--motion-easing),
              background var(--motion-duration) var(--motion-easing);
}
.back-btn:hover { color: var(--color-fg); background: var(--color-hover); }
.back-btn:focus-visible { outline: 2px solid var(--color-primary); outline-offset: 1px; }
.detail-title {
  font: var(--fw-body) var(--fs-body)/var(--lh-body) var(--font-body);
  color: var(--color-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.divider { height: 1px; background: var(--color-border); margin: 0 var(--gap); }
.detail-scroll { flex: 1; overflow-y: auto; padding: var(--gap) 0 var(--pad-y); }
.detail-scroll::-webkit-scrollbar { width: 6px; }
.detail-scroll::-webkit-scrollbar-thumb { background: var(--color-border); border-radius: 3px; }
.summary { padding: 0 var(--pad-x); }
.sum-row { display: flex; flex-wrap: wrap; gap: var(--gap) var(--pad-x); }
.sum-row.sub {
  margin-top: 6px;
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-body);
  color: var(--color-muted);
}
.sum { display: inline-flex; align-items: baseline; gap: 4px; }
.sum b {
  font: var(--fw-body) var(--fs-body)/var(--lh-body) var(--font-utility);
  font-variant-numeric: tabular-nums;
  color: var(--color-fg);
}
.sum i {
  font-style: normal;
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body);
  color: var(--color-tertiary);
}
.turns-head {
  padding: 10px var(--pad-x) 4px;
  font: 600 var(--fs-caption)/var(--lh-caption) var(--font-utility);
  color: var(--color-muted);
  letter-spacing: 0.05em;
  text-transform: uppercase;
}
.turns { list-style: none; margin: 0; padding: 0; }
.turn {
  display: flex;
  align-items: center;
  gap: var(--gap);
  padding: 4px var(--pad-x);
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-body);
}
.turn:hover { background: var(--color-hover); }
.t-idx { color: var(--color-tertiary); flex-shrink: 0; font-variant-numeric: tabular-nums; width: 28px; }
.t-prompt {
  flex: 1; min-width: 0;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  color: var(--color-fg);
}
.t-tok {
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
  color: var(--color-muted);
}
.t-tok .arr { opacity: 0.6; margin: 0 1px; }
.t-tools { flex-shrink: 0; color: var(--color-tertiary); }
.t-ago { flex-shrink: 0; color: var(--color-tertiary); font-variant-numeric: tabular-nums; }
</style>
```

- [ ] **Step 3: PanelView.vue — import + 状态**

顶部 script import 区加（在 `import StatusIcon` 附近）：

```ts
import DetailPanel from './DetailPanel.vue';
import type { SessionDetail } from '../types';
```

在 `const all = ref<Session[]>([]);` 附近加状态：

```ts
// 详情子状态：selectedDetail 非空时切到 DetailPanel（不动全局 mode/窗口）。
const selectedDetail = ref<SessionDetail | null>(null);
const detailName = ref('');

async function openDetail(s: Session) {
  detailName.value = s.name || s.project;
  try {
    selectedDetail.value = await invoke<SessionDetail>('get_session_detail', { id: s.id });
  } catch (e) {
    console.error('get_session_detail failed', e);
  }
}
```

- [ ] **Step 4: PanelView.vue — 模板最外层切换**

把 `<template>` 最外层 `<div class="overlay">` 内部，原 `<div class="search-bar">...` 到 `<div class="list-scroll">...</div>` 整段用 `<template v-if="!selectedDetail">` 包裹，并在前面加 DetailPanel：

```html
  <div class="overlay">
    <DetailPanel
      v-if="selectedDetail"
      :detail="selectedDetail"
      :name="detailName"
      @back="selectedDetail = null"
    />
    <template v-else>
      <!-- 原 search-bar + divider + list-scroll 三段，原样保留 -->
    </template>
  </div>
```

（即将原 `.search-bar`、`.divider`、`.list-scroll` 三个直接子元素整体包进 `<template v-else>`。）

- [ ] **Step 5: PanelView.vue — actions 加详情按钮（图标）**

两处 `.actions`（搜索态 + 非搜索态）的最前面（搁置按钮之前）各加一个详情按钮：

```html
            <button
              class="act-btn detail"
              title="详情"
              aria-label="详情"
              @click.stop="openDetail(s)"
            >
              <svg class="ico" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M3 3v18h18" /><path d="M7 16v-5" /><path d="M12 16V8" /><path d="M17 16v-3" />
              </svg>
            </button>
```

- [ ] **Step 6: PanelView.vue — act-btn 图标 CSS**

`.act-btn` 规则内补一行让 SVG 居中（其余 padding/border 不变）：

```css
.act-btn {
  /* …现有属性不变… */
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.act-btn .ico { display: block; }
```

- [ ] **Step 7: Verify**

Run: `npm run tauri dev`
Expected: overlay 任一会话行点"详情"（柱状图图标）→ 切到详情视图，显示汇总（输入/输出/缓存命中 + 模型/回合/工具）+ 按回合列表；点 ← 返回回列表。

- [ ] **Step 8: Commit**

```bash
git add src/types.ts src/components/DetailPanel.vue src/components/PanelView.vue
git commit -m "feat: 会话 token 详情视图（汇总 + 按回合，PanelView 子状态）"
```

---

### Task 5: 前端 — actions 其余按钮图标化

**Files:**
- Modify: `src/components/PanelView.vue`
- Verify: `npm run tauri dev`，看 actions 全是图标

**Interfaces:**
- 无新接口；仅把现有文字按钮内容换成 SVG（详情按钮已在 Task 4 图标化）。

- [ ] **Step 1: 搁置/恢复按钮改图标**

两处 `.actions`（搜索态 + 非搜索态）的搁置/恢复按钮，文字内容换成 SVG。`恢复` 按钮：

```html
            <button
              v-if="s.alive && s.snoozed"
              class="act-btn snooze"
              title="恢复（取消搁置）"
              aria-label="恢复（取消搁置）"
              @click.stop="unsnooze(s.id)"
            >
              <svg class="ico" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
              </svg>
            </button>
```

`搁置` 按钮（v-else-if 那个）：

```html
            <button
              v-else-if="s.alive && (s.status === 'waitingForInput' || s.status === 'waitingForReply')"
              class="act-btn snooze"
              title="搁置（暂时不管）"
              aria-label="搁置（暂时不管）"
              @click.stop="snooze(s.id)"
            >
              <svg class="ico" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" />
              </svg>
            </button>
```

- [ ] **Step 2: 归档/取消归档按钮改图标**

两处归档按钮换成 SVG（按 `archived.has(s.id)` 切 archive / archive-restore）：

```html
            <button
              class="act-btn archive"
              :title="archived.has(s.id) ? '取消归档' : '归档'"
              :aria-label="archived.has(s.id) ? '取消归档' : '归档'"
              @click.stop="archived.has(s.id) ? unarchive(s.id) : archive(s.id)"
            >
              <svg v-if="archived.has(s.id)" class="ico" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <rect x="2" y="3" width="20" height="5" rx="1" /><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" /><path d="M12 18v-6" /><path d="M9 15l3-3 3 3" />
              </svg>
              <svg v-else class="ico" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <rect x="2" y="3" width="20" height="5" rx="1" /><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" /><path d="M10 12h4" />
              </svg>
            </button>
```

- [ ] **Step 3: 复制按钮改图标**

两处复制按钮换成 SVG（按 `copiedId === s.id` 切 check / copy）：

```html
            <button
              class="act-btn copy"
              :class="{ done: copiedId === s.id }"
              :title="copiedId === s.id ? '已复制' : '复制'"
              :aria-label="copiedId === s.id ? '已复制' : '复制'"
              @click.stop="copyId(s.id)"
            >
              <svg v-if="copiedId === s.id" class="ico" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M20 6 9 17l-5-5" />
              </svg>
              <svg v-else class="ico" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
              </svg>
            </button>
```

- [ ] **Step 4: Verify**

Run: `npm run tauri dev`
Expected: overlay 每个 alive 会话行 actions 全是图标（详情/搁置或恢复/归档/复制），hover 有 title 提示，复制成功显示 ✓；视觉与 pin/collapse 统一。

- [ ] **Step 5: Commit**

```bash
git add src/components/PanelView.vue
git commit -m "refactor: actions 文字按钮改 Lucide 图标（搁置/归档/复制）"
```

---

## Self-Review 记录

（写完后自查：spec 每节均有任务覆盖——A=Task1+2、B=Task3+4、C=Task4详情按钮+Task5；无 placeholder；类型名前后一致 `tokensIn/tokensOut/cacheRead/turnCount/toolCalls/webSearches/webFetches`、`scan_session_jsonl`/`scan_session_detail`/`scan_detail_from_text`；回合 tool_result 不开新回合有专项测试。）
