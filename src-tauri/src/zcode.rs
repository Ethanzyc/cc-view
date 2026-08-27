// ZCode 桌面 App 会话采集：数据源是 ~/.zcode/cli/db/db.sqlite（WAL 模式 SQLite）。
// 不引入 rusqlite，走系统 /usr/bin/sqlite3 -readonly -json 只读查询，
// 与 collector.rs 的子命令采集模式一致：db 缺失 / spawn 失败 / 输出异常一律空结果隔离。
// token 口径按 zcode 服务端报告值（bigmodel anthropic 兼容层 totalTokens = input+output，
// input 已含 cache read），故 tokens 只取原始列、不再叠加 cache 字段。
use crate::models::{FocusHint, Host, Session, SessionDetail, Source, Status, TurnStat};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SQLITE3: &str = "/usr/bin/sqlite3";

/// zcode 会话没有进程级存活信号（GUI 内嵌 cli），按最近活动判活：
/// time_updated 在窗口内或仍有 running turn 视为活跃。窗口越长会话沉底越慢。
const ALIVE_WINDOW_MS: i64 = 15 * 60 * 1000;

/// 主会话列表 + model_usage token 累计。排除子代理会话与 zcode 侧已归档。
/// COALESCE 兜 LEFT JOIN 无用量记录的会话。
const SESSIONS_SQL: &str = r#"
SELECT s.id            AS id,
       s.directory     AS cwd,
       s.title         AS title,
       s.slug          AS slug,
       s.time_created  AS time_created,
       s.time_updated  AS time_updated,
       s.time_compacting AS time_compacting,
       COALESCE(t.tin, 0)  AS tokens_in,
       COALESCE(t.tout, 0) AS tokens_out
FROM session s
LEFT JOIN (
    SELECT session_id,
           SUM(input_tokens)  AS tin,
           SUM(output_tokens) AS tout
    FROM model_usage
    GROUP BY session_id
) t ON t.session_id = s.id
WHERE s.parent_id IS NULL AND s.time_archived IS NULL
"#;

/// 正在跑模型请求 / 回合的会话 id 并集——Working 判定依据。
const RUNNING_SQL: &str = r#"
SELECT DISTINCT session_id AS session_id FROM model_usage WHERE status = 'running'
UNION
SELECT DISTINCT session_id FROM turn_usage WHERE status = 'running'
"#;

#[derive(Debug, Deserialize)]
struct ZcSessionRow {
    id: String,
    cwd: String,
    title: String,
    slug: Option<String>,
    time_created: i64,
    time_updated: i64,
    time_compacting: Option<i64>,
    #[serde(default)]
    tokens_in: u64,
    #[serde(default)]
    tokens_out: u64,
}

#[derive(Debug, Deserialize)]
struct ZcRunningRow {
    session_id: String,
}

pub fn db_path(home: &Path) -> std::path::PathBuf {
    home.join(".zcode/cli/db/db.sqlite")
}

/// 一轮采集全部可见的 ZCode 会话。
pub fn collect(home: &Path) -> Vec<Session> {
    let db = db_path(home);
    if !db.is_file() {
        return vec![];
    }
    let Some(rows) = query(&db, SESSIONS_SQL).map(parse_rows::<ZcSessionRow>) else {
        return vec![];
    };    let running: HashSet<String> = query(&db, RUNNING_SQL)
        .map(|txt| {
            parse_rows::<ZcRunningRow>(txt)
                .into_iter()
                .map(|r| r.session_id)
                .collect()
        })
        .unwrap_or_default();

    // now_ms 每轮取一次：同一轮内所有会话的新鲜度基准一致
    let now_ms = unix_now_ms();
    rows.into_iter()
        .filter_map(|r| build_session(r, &running, now_ms))
        .collect()
}

fn build_session(r: ZcSessionRow, running: &HashSet<String>, now_ms: i64) -> Option<Session> {
    let is_running = running.contains(&r.id);
    // 标题三级回退：title > slug > 目录名（空串视为缺失，避免列表出现空名）
    let project = basename(&r.cwd);
    let name = if !r.title.trim().is_empty() {
        r.title
    } else {
        r.slug.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| project.clone())
    };
    let alive = is_running || now_ms - r.time_updated <= ALIVE_WINDOW_MS;
    let status = if is_running {
        Status::Working
    } else if r.time_compacting.is_some() {
        Status::Compacting
    } else {
        Status::WaitingForInput
    };
    Some(Session {
        id: r.id,
        source: Source::Zcode,
        pid: 0,
        project,
        cwd: r.cwd,
        name,
        status,
        started_at: r.time_created,
        status_updated_at: r.time_updated,
        alive,
        focus_hint: FocusHint {
            host: Host::ZcodeApp,
            ..FocusHint::default()
        },
        snoozed: false,
        tokens_in: r.tokens_in,
        tokens_out: r.tokens_out,
    })
}

// ---- 详情（on-demand：点击会话行才查）----

#[derive(Debug, Deserialize)]
struct ZcTurnRow {
    user_message_id: Option<String>,
    iso: Option<String>,
    #[serde(default)]
    tool_calls: u64,
    #[serde(default)]
    tokens_in: u64,
    #[serde(default)]
    tokens_out: u64,
}

#[derive(Debug, Deserialize)]
struct ZcSummaryRow {
    #[serde(default)]
    tokens_in: u64,
    #[serde(default)]
    tokens_out: u64,
    #[serde(default)]
    cache_read: u64,
    #[serde(default)]
    cache_creation: u64,
    #[serde(default)]
    tool_calls: u64,
    #[serde(default)]
    turn_count: u64,
    #[serde(default)]
    ctx_peak: u64,
}

#[derive(Debug, Deserialize)]
struct ZcModelRow {
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ZcPromptRow {
    mid: String,
    #[serde(default)]
    txt: Option<String>,
}

/// 会话详情：turn_usage 聚合 + 每 turn 的用户输入前缀。查询失败返回 None（前端不显示详情）。
pub fn detail(home: &Path, session_id: &str) -> Option<SessionDetail> {
    let db = db_path(home);
    if !db.is_file() || !safe_id(session_id) {
        return None;
    }
    // 详情接口是 on-demand 单次调用，三个查询各一次 spawn 可接受
    let sid = quote_js(session_id);
    let turns =
        parse_rows::<ZcTurnRow>(query(&db, &with_sid(TURN_USAGE_SQL_TPL, &sid))?);
    // 无任何回合记录 = 该 id 在 zcode 库中没有数据（或已被清理），按缺失处理
    if turns.is_empty() {
        return None;
    }
    let summary =
        parse_rows::<ZcSummaryRow>(query(&db, &with_sid(SUMMARY_SQL_TPL, &sid))?)
            .into_iter()
            .next()?;
    let model =
        parse_rows::<ZcModelRow>(query(
            &db,
            &with_sid(
                "SELECT model_id AS model FROM model_usage WHERE session_id = '{}' ORDER BY started_at DESC LIMIT 1",
                &sid,
            ),
        )?)
        .into_iter()
        .next()
        .and_then(|m| m.model);
    // 每个 user message 取首个非空 text part 作为回合 prompt 前缀。
    // prompt 只是增强信息：查询失败降级为空，不毁掉整个详情。
    let mut first_text: HashMap<String, String> = HashMap::new();
    if let Some(out) = query(&db, &with_sid(PROMPTS_SQL_TPL, &sid)) {
        for p in parse_rows::<ZcPromptRow>(out) {
            if let Some(txt) = nonempty(p.txt) {
                first_text.entry(p.mid).or_insert(txt);
            }
        }
    }

    let last_input_turns: Vec<u64> = turns.iter().map(|t| t.tokens_in).collect();
    let ctx_current = last_input_turns.last().copied().unwrap_or(0);
    let turn_stats: Vec<TurnStat> = turns
        .iter()
        .enumerate()
        .map(|(i, t)| TurnStat {
            idx: i as u32 + 1,
            prompt: t
                .user_message_id
                .as_ref()
                .and_then(|mid| first_text.get(mid))
                .map(|txt| truncate_chars(txt, 40))
                .unwrap_or_default(),
            tokens_in: t.tokens_in,
            tokens_out: t.tokens_out,
            tool_calls: t.tool_calls as u32,
            ctx: t.tokens_in,
            ts: t.iso.clone().unwrap_or_default(),
        })
        .collect();

    Some(SessionDetail {
        session_id: session_id.to_string(),
        tokens_in: summary.tokens_in,
        tokens_out: summary.tokens_out,
        cache_read: summary.cache_read,
        cache_creation: summary.cache_creation,
        model: model.unwrap_or_else(|| "unknown".to_string()),
        turn_count: summary.turn_count as u32,
        tool_calls: summary.tool_calls as u32,
        web_searches: 0,
        web_fetches: 0,
        context_current: ctx_current,
        context_peak: summary.ctx_peak.max(ctx_current),
        compact_count: 0,
        turns: turn_stats,
    })
}

const TURN_USAGE_SQL_TPL: &str = r#"
SELECT user_message_id AS user_message_id,
       strftime('%Y-%m-%dT%H:%M:%fZ', started_at/1000.0, 'unixepoch') AS iso,
       tool_call_count AS tool_calls,
       input_tokens AS tokens_in,
       output_tokens AS tokens_out
FROM turn_usage WHERE session_id = '{}' ORDER BY started_at
"#;

const SUMMARY_SQL_TPL: &str = r#"
SELECT COALESCE(SUM(input_tokens), 0)              AS tokens_in,
       COALESCE(SUM(output_tokens), 0)             AS tokens_out,
       COALESCE(SUM(cache_read_input_tokens), 0)   AS cache_read,
       COALESCE(SUM(cache_creation_input_tokens), 0) AS cache_creation,
       COALESCE(SUM(tool_call_count), 0)           AS tool_calls,
       COUNT(DISTINCT turn_id)                     AS turn_count,
       COALESCE(MAX(input_tokens), 0)              AS ctx_peak
FROM turn_usage WHERE session_id = '{}'
"#;

const PROMPTS_SQL_TPL: &str = r#"
SELECT p.message_id AS mid,
       json_extract(p.data, '$.text') AS txt
FROM part p JOIN message m ON m.id = p.message_id
WHERE p.session_id = '{}' AND json_extract(m.data, '$.role') = 'user'
ORDER BY m.time_created, p.sequence
"#;

// ---- sqlite3 CLI 封装 ----

/// 跑一条只读 SQL 返回 stdout 文本。sqlite3 正常 <50ms，5s deadline 防 WAL 忙等挂死轮询线程。
fn query(db: &Path, sql: &str) -> Option<String> {
    let mut child = match Command::new(SQLITE3)
        .args(["-readonly", "-json"])
        .arg(db)
        .arg(sql)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("zcode: failed to spawn sqlite3: {}", e);
            return None;
        }
    };
    use std::io::Read;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut stdout);
                }
                if !status.success() {
                    log::warn!("zcode: sqlite3 exited with {status}");
                    return None;
                }
                return Some(stdout);
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait(); // reap zombie
                log::warn!("zcode: sqlite3 timed out after 5s");
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                log::warn!("zcode: sqlite3 wait error: {}", e);
                return None;
            }
        }
    }
}

fn parse_rows<T: for<'de> Deserialize<'de>>(out: String) -> Vec<T> {
    if out.trim().is_empty() {
        return vec![]; // -json 对零行输出可能为空串
    }
    serde_json::from_str::<Vec<T>>(&out).unwrap_or_else(|e| {
        log::warn!("zcode: bad sqlite json output: {}", e);
        vec![]
    })
}

/// id 只允许出现在 zcode db 里的字符；防拼接注入（fail fast）。
fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 单引号翻倍兜底（与 safe_id 双保险）。
fn quote_js(s: &str) -> String {
    s.replace('\'', "''")
}

/// SQL 模板占位注入（format! 不接受常量字符串作格式串，故手写替换）。
fn with_sid(tpl: &str, sid: &str) -> String {
    tpl.replace("{}", sid)
}

fn basename(path: &str) -> String {
    path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path).to_string()
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.filter(|t| !t.trim().is_empty())
}

fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_available() -> bool {
        Path::new(SQLITE3).exists()
    }

    /// 建最小 fixture 库（session/model_usage/turn_usage/message/part 所需列与线上 schema 同名子集）。
    fn fixture_db(dir: &Path) -> std::path::PathBuf {
        let db = dir.join("db.sqlite");
        let ddl = r#"
CREATE TABLE session (
    id text primary key, parent_id text, directory text not null,
    title text not null, slug text not null,
    time_created integer not null, time_updated integer not null,
    time_compacting integer, time_archived integer
);
CREATE TABLE model_usage (
    session_id text not null, turn_id text, model_id text, status text not null,
    started_at integer, input_tokens integer default 0, output_tokens integer default 0,
    cache_read_input_tokens integer default 0, cache_creation_input_tokens integer default 0
);
CREATE TABLE turn_usage (
    session_id text not null, turn_id text not null, user_message_id text,
    status text not null, started_at integer,
    tool_call_count integer default 0, input_tokens integer default 0, output_tokens integer default 0,
    cache_read_input_tokens integer default 0, cache_creation_input_tokens integer default 0,
    primary key(session_id, turn_id)
);
CREATE TABLE message (id text primary key, session_id text not null, data text not null, time_created integer, sequence integer);
CREATE TABLE part (id text primary key, message_id text not null, session_id text not null, data text not null, sequence integer);
INSERT INTO session VALUES
 ('sess_aaa', NULL, '/Users/x/proj-a', '修复登录页', 'fix-login', 1000000, 9000000, NULL, NULL),
 ('sess_arch', NULL, '/Users/x/proj-a', '已归档', 'arch', 1, 2, NULL, 9999),
 ('sess_sub', 'sess_aaa', '/Users/x/proj-a', '子代理', 'sub', 3, 4, NULL, NULL),
 ('sess_idle', NULL, '/Users/x/deep/dir-b', '', '', 5000000, 6000000, 5555555, NULL);
INSERT INTO model_usage VALUES
 ('sess_aaa', 't1', 'GLM-5.3', 'completed', 8000000, 200, 30, 150, 10),
 ('sess_aaa', 't2', 'GLM-5.3', 'running', 8500000, 400, 50, 300, 20),
 ('sess_idle', 't3', 'GLM-5.3-Flash', 'completed', 5500000, 90, 8, 80, 1);
INSERT INTO turn_usage VALUES
 ('sess_aaa', 't1', 'msg_u1', 'completed', 7000000, 2, 200, 30, 150, 10),
 ('sess_aaa', 't2', 'msg_u2', 'running', 8500000, 1, 400, 50, 300, 20),
 ('sess_idle', 't3', 'msg_u3', 'completed', 5500000, 0, 90, 8, 80, 1);
INSERT INTO message VALUES
 ('msg_u1', 'sess_aaa', '{"role":"user"}', 7000000, 0),
 ('msg_u2', 'sess_aaa', '{"role":"user"}', 8500000, 1),
 ('msg_p1', 'sess_aaa', '{"role":"assistant"}', 8600000, 2);
INSERT INTO part VALUES
 ('pa1', 'msg_u1', 'sess_aaa', '{"type":"text","text":"帮我修复登录页的白屏问题"}', 0),
 ('pa2', 'msg_u1', 'sess_aaa', '{"type":"text","text":"补充信息"}', 1),
 ('pb1', 'msg_u2', 'sess_aaa', '{"type":"text","text":"继续,谢谢"}', 0);
"#;
        let out = Command::new(SQLITE3).arg(&db).arg(ddl).output().expect("run sqlite3 ddl");
        assert!(out.status.success(), "fixture ddl failed: {}", String::from_utf8_lossy(&out.stderr));
        db
    }

    #[test]
    fn collect_filters_and_maps() {
        if !sqlite_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let db = fixture_db(tmp.path());
        // collect 以 home 推导路径 → 把 fixture 放进 home/.zcode/cli/db/
        let fake_home = tmp.path().join("home");
        let real_db = db_path(Path::new(&fake_home));
        std::fs::create_dir_all(real_db.parent().unwrap()).unwrap();
        std::fs::copy(&db, &real_db).unwrap();

        let sessions = collect(&fake_home);
        // sess_arch 已归档、sess_sub 是子代理 → 均不可见
        assert_eq!(sessions.len(), 2);
        let a = sessions.iter().find(|s| s.id == "sess_aaa").unwrap();
        assert_eq!(a.name, "修复登录页");
        assert_eq!(a.project, "proj-a");
        assert_eq!(a.source, Source::Zcode);
        assert!(matches!(a.status, Status::Working)); // 有 running turn
        assert!(a.alive);
        assert_eq!(a.focus_hint.host, Host::ZcodeApp);
        assert_eq!(a.tokens_in, 600); // 200+400
        assert_eq!(a.tokens_out, 80);

        let idle = sessions.iter().find(|s| s.id == "sess_idle").unwrap();
        // 无 running：time_updated 在 15min 窗口外 → dead
        assert!(!idle.alive);
        assert!(matches!(idle.status, Status::Compacting)); // time_compacting 非空
        assert_eq!(idle.name, "dir-b"); // title/slug 全空回退目录名
    }

    #[test]
    fn detail_builds_turns_and_prompts() {
        if !sqlite_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let fake_home = tmp.path().join("home");
        let real_db = db_path(Path::new(&fake_home));
        std::fs::create_dir_all(real_db.parent().unwrap()).unwrap();
        std::fs::copy(fixture_db(tmp.path()), &real_db).unwrap();

        let d = detail(&fake_home, "sess_aaa").expect("detail sess_aaa");
        assert_eq!(d.turn_count, 2);
        assert_eq!(d.tokens_in, 600);
        assert_eq!(d.tokens_out, 80);
        assert_eq!(d.cache_read, 450); // 150+300
        assert_eq!(d.cache_creation, 30);
        assert_eq!(d.tool_calls, 3);
        assert_eq!(d.model, "GLM-5.3"); // 最新一条 model_usage
        assert_eq!(d.context_peak, 400);
        assert_eq!(d.context_current, 400); // 最后一个 turn 的 input
        // 回合按 started_at 排序；prompt 取该 user message 首个 text part
        let t1 = &d.turns[0];
        assert_eq!(t1.idx, 1);
        assert_eq!(t1.prompt, "帮我修复登录页的白屏问题");
        assert_eq!(t1.ts, "1970-01-01T01:56:40.000Z"); // started_at 7000000ms → UTC
        assert_eq!(&d.turns[1].prompt, "继续,谢谢");

        // 注入防护：单引号直接拒绝；纯函数行为
        assert!(detail(&fake_home, "x'--").is_none());
        assert!(detail(&fake_home, "sess_missing").is_none());
        assert!(safe_id("sess_294dbba9-da07-4bf6"));
        assert_eq!(truncate_chars("你好世界", 2), "你好");
        assert_eq!(basename("/Users/x/proj-a/"), "proj-a");
        assert_eq!(basename("relative"), "relative");
    }

    /// 真机 smoke：对本机 ~/.zcode 数据库全链路跑通（需要已装 ZCode 才有意义，
    /// 且依赖个人数据 → 默认 ignore，手动 cargo test -- --ignored 验证）。
    #[test]
    #[ignore]
    fn real_db_smoke() {
        let Some(home) = dirs::home_dir() else { return };
        if !db_path(&home).is_file() {
            eprintln!("no zcode db on this machine; skip");
            return;
        }
        let sessions = collect(&home);
        eprintln!("collect => {} sessions", sessions.len());
        for s in sessions.iter().take(5) {
            eprintln!(
                "  [{:?}/{:?}] {} | cwd={} | tok in/out = {}/{}",
                s.source, s.status, s.name, s.cwd, s.tokens_in, s.tokens_out
            );
        }
        assert!(!sessions.is_empty(), "expected real sessions from local db");
        let first = &sessions[0];
        assert_eq!(first.source, Source::Zcode);
        assert_eq!(first.focus_hint.host, Host::ZcodeApp);
        // 详情链路：拿列表里第一个能出详情的会话
        let with_detail = sessions.iter().find_map(|s| detail(&home, &s.id));
        assert!(with_detail.is_some(), "expected at least one session with turns");
        let d = with_detail.unwrap();
        eprintln!("detail => model={} turns={} tool_calls={}", d.model, d.turn_count, d.tool_calls);
    }
}
