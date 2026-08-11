// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod archived;
mod badge;
mod collector;
mod discovery;
mod focus;
mod liveness;
mod models;
mod notify;
mod overlay_position;
mod permission;
mod prefs;
mod reducer;
mod snoozed;
mod statemachine;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{Emitter, Manager};

/// 基于 SnoozeMap 给 merged 的每个 session 算 derived snoozed 字段。
/// map 为 None 或 lock 失败时静默（视为无搁置）——不阻塞调用方。
/// 锁一次 guard，遍历所有 session 共用（避免每 session 各 lock 一次）。
fn apply_snoozed(
    merged: &[models::Session],
    map: Option<&Mutex<snoozed::SnoozeMap>>,
) -> Vec<models::Session> {
    let guard = map.and_then(|m| m.lock().ok());
    merged
        .iter()
        .map(|s| {
            let mut s = s.clone();
            s.snoozed = guard
                .as_ref()
                .map(|g| g.is_effectively_snoozed(&s))
                .unwrap_or(false);
            s
        })
        .collect()
}

/// 对会话列表做内容 hash，仅按 (id, status, alive, snoozed) 维度。
/// 只要这四项不变就不 emit，避免每 3s 给前端刷一屏。
/// 含 snoozed：derived 字段 true→false（自动冒泡）时即便 (id,status,alive) 不变也得 emit，
/// 否则前端滞留灰显、违背 spec 3.1"有新动静自动冒泡"。
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

/// 单色 menubar 剪影（template image：黑 + 透明）。
/// include_bytes 编译期嵌入，运行时无需读盘；改图后需重新编译。
const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");

/// 常驻窗口宽度（logical px）。A 极简最窄，B 精简需容纳"名称 + 状态中文"。
const RESIDENT_WIDTH_A: f64 = 180.0;
const RESIDENT_WIDTH_B: f64 = 285.0;

fn resident_layout_width(layout: prefs::ResidentLayout) -> f64 {
    match layout {
        prefs::ResidentLayout::A => RESIDENT_WIDTH_A,
        prefs::ResidentLayout::B => RESIDENT_WIDTH_B,
    }
}

/// 右边锚定几何：宽度从 old_w 变 new_w 时的新 x（保证 old_x + old_w == new_x + new_w，
/// 即窗口右边缘不动）。set_resident_width 与启动恢复共用。
fn anchored_x(old_x: f64, old_w: f64, new_w: f64) -> f64 {
    old_x + old_w - new_w
}

/// 纯函数：physical 坐标 (px,py) 是否在 rect (x0,y0,w,h) 内。
/// 供窗口位置恢复校验——防坏坐标（如宽度 bug 残留的屏外坐标）恢复到屏外看不见。
fn pos_in_rect(px: i32, py: i32, x0: i32, y0: i32, w: i32, h: i32) -> bool {
    px >= x0 && px < x0 + w && py >= y0 && py < y0 + h
}

/// 面板模式窗口尺寸（logical px，与 tauri.conf.json overlay width/height 一致）。
const PANEL_W: f64 = 560.0;
const PANEL_H: f64 = 420.0;

/// macOS 几何类型（手 impl Encode，避免拉 objc2-foundation 整包）。
/// 64 位 NSRect = CGRect（同布局），@encode 名用 CGRect（与系统一致）。
#[repr(C)]
#[derive(Copy, Clone)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Copy, Clone)]
struct CGSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Copy, Clone)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}
// SAFETY: repr(C)，encoding 与系统 CGRect 一致（见 objc2 encode_core_graphics 示例）。
unsafe impl objc2::encode::Encode for CGPoint {
    const ENCODING: objc2::encode::Encoding =
        objc2::encode::Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl objc2::encode::Encode for CGSize {
    const ENCODING: objc2::encode::Encoding =
        objc2::encode::Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl objc2::encode::Encode for CGRect {
    const ENCODING: objc2::encode::Encoding =
        objc2::encode::Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}

/// 模式切换动画进行中标志：set_resident_height 期间跳过，避免与动画 set_size 冲突。
static ANIMATING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// mode 切换时间戳（ms）：Focused(false) 在切换后短时间内忽略，避免动画期间
/// 窗口 resign key 抖动触发 hide（「常驻→放大闪消失」根因）。
static LAST_MODE_CHANGE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Moved debounce：最新待落盘坐标 + 最后移动时间戳 + 是否已有 debounce 线程在跑。
/// 拖动时 Moved ~60Hz，直接每次落盘 = IO 风暴；debounce 静止 300ms 后落一次。
static PENDING_MOVE_POS: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);
static LAST_MOVE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DEBOUNCE_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 用 macOS 原生 setFrame:display:animate: 做窗口缩放/移动动画（Core Animation，渲染层平滑）。
/// 手动逐帧 set_size + set_position 每帧两次 NSWindow setFrame + webview reflow，顿挫明显；
/// 原生动画一次 setFrame、系统在渲染层插值，顺很多。系统动画异步（~0.25s），ANIMATING 在
/// 估计时长后清除，期间 set_resident_height 跳过；清除后 ResizeObserver 重触发校正高度。
#[cfg(target_os = "macos")]
fn animate_window_to(app: &tauri::AppHandle, tw: f64, th: f64, tx: f64, ty: f64) {
    use objc2::{class, msg_send, runtime::AnyObject};
    let Some(w) = app.get_webview_window("overlay") else {
        return;
    };
    // Cocoa 坐标原点在屏幕左下：NS y = 屏幕高度(logical) - Tauri y - 窗口高。
    let screen_h = w
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| m.size().height as f64 / m.scale_factor())
        .unwrap_or(800.0);
    let rect = CGRect {
        origin: CGPoint {
            x: tx,
            y: screen_h - ty - th,
        },
        size: CGSize {
            width: tw,
            height: th,
        },
    };
    let Ok(ptr) = w.ns_window() else { return };
    let obj = ptr as *mut AnyObject;

    ANIMATING.store(true, std::sync::atomic::Ordering::Relaxed);
    unsafe {
        // 切换前 activate app + makeKey：常驻→放大时若 app 不 active（点过别的应用），
        // 动画后窗口会 resign key 触发失焦 hide。主动激活避免。
        let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![ns_app, activateIgnoringOtherApps: objc2::runtime::Bool::YES];
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![obj, makeKeyAndOrderFront: nil];
        let yes = objc2::runtime::Bool::YES;
        let _: () = msg_send![obj, setFrame: rect, display: yes, animate: yes];
    }
    let app_handle = app.clone();
    std::thread::spawn(move || {
        // spinner 总时长 ≈ 50ms（就位）+ 150ms = 200ms 露目标视图。
        std::thread::sleep(std::time::Duration::from_millis(50));
        ANIMATING.store(false, std::sync::atomic::Ordering::Relaxed);
        // 通知前端动画结束：精确退出过渡态（比前端兜底 setTimeout 准）。
        let _ = app_handle.emit("animate_done", ());
    });
}

/// 把 overlay 窗口尺寸 + 位置（动画）切到目标模式。
/// - panel：560×420，居中。
/// - resident：宽度按 layout，高度沿用当前（动画结束后前端 set_resident_height 校正到内容高度），右上角。
fn apply_mode_window(
    app: &tauri::AppHandle,
    mode: prefs::OverlayMode,
    layout: prefs::ResidentLayout,
) {
    let resident_width = if let Some(state) = app.try_state::<Mutex<prefs::Prefs>>() {
        state.lock().ok().map(|p| p.resident_width).unwrap_or(None)
    } else {
        None
    };
    let Some(w) = app.get_webview_window("overlay") else {
        return;
    };
    let sf = w.scale_factor().ok().unwrap_or(1.0);
    let mon = w.current_monitor().ok().flatten();

    let (tw, th, tx, ty) = match mode {
        prefs::OverlayMode::Panel => {
            let (cx, cy) = mon
                .map(|m| {
                    let mw = m.size().width as f64 / sf;
                    let mh = m.size().height as f64 / sf;
                    let mx = m.position().x as f64 / sf;
                    let my = m.position().y as f64 / sf;
                    (mx + (mw - PANEL_W) / 2.0, my + (mh - PANEL_H) / 2.0)
                })
                .unwrap_or((0.0, 0.0));
            (PANEL_W, PANEL_H, cx, cy)
        }
        prefs::OverlayMode::Resident => {
            let width = resident_width.unwrap_or_else(|| resident_layout_width(layout));
            let cur_h = w
                .outer_size()
                .and_then(|s| w.scale_factor().map(|sf2| s.to_logical::<f64>(sf2).height))
                .unwrap_or(PANEL_H);
            let (rx, ry) = mon
                .map(|m| {
                    let mw = m.size().width as f64 / sf;
                    let mx = m.position().x as f64 / sf;
                    let my = m.position().y as f64 / sf;
                    (mx + mw - width - 8.0, my + 28.0 + 4.0)
                })
                .unwrap_or((0.0, 0.0));
            (width, cur_h, rx, ry)
        }
    };
    animate_window_to(app, tw, th, tx, ty);
}

/// 一次 setFrame 更新常驻窗口宽+位置（右边锚定），不闪。仅 macOS。
/// 高度沿用当前；x 由 anchored_x 算（右边缘不动）。窗口不存在/ns_window 失败静默返回。
#[cfg(target_os = "macos")]
fn set_resident_window_width(app: &tauri::AppHandle, new_width: f64) {
    use objc2::{msg_send, runtime::AnyObject};
    let Some(w) = app.get_webview_window("overlay") else {
        return;
    };
    let Ok(ptr) = w.ns_window() else { return };
    let sf = w.scale_factor().ok().unwrap_or(1.0);
    let pos = w.outer_position().ok();
    let size = w.outer_size().ok();
    let (Some(pos), Some(size)) = (pos, size) else {
        return;
    };
    // outer_position/size 返回 physical；setFrame 要 logical（point）——统一转 logical。
    // 旧版 old_x=pos.x（physical）与 old_w（logical）单位混用，retina 下 new_x 偏大 → 窗口移出屏。
    let pos_l = pos.to_logical::<f64>(sf);
    let old_x = pos_l.x;
    let old_w = size.to_logical::<f64>(sf).width;
    let old_h = size.to_logical::<f64>(sf).height;
    let new_x = anchored_x(old_x, old_w, new_width);
    // NS 坐标原点左下：y = screen_h - top_y - height
    let screen_h = w
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| m.size().height as f64 / m.scale_factor())
        .unwrap_or(800.0);
    let rect = CGRect {
        origin: CGPoint {
            x: new_x,
            y: screen_h - pos_l.y - old_h,
        },
        size: CGSize {
            width: new_width,
            height: old_h,
        },
    };
    let obj = ptr as *mut AnyObject;
    unsafe {
        let yes = objc2::runtime::Bool::YES;
        let no = objc2::runtime::Bool::NO;
        let _: () = msg_send![obj, setFrame: rect, display: yes, animate: no];
    }
}

/// 后台线程：每 3s collect → reduce → notify → hash 去重 → 仅变化时 emit("sessions")。
/// 同时每轮聚合 need_attention/working 计数 → tray.set_tooltip + set_icon。
fn start_poll_loop(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut last_hash = 0u64;
        // 线程私有 notifier——observe 每轮调用以维护状态迁移
        let mut notifier = notify::Notifier::new();
        // 加载嵌入的单色剪影（template image）。
        // tauri::image::Image::from_bytes 解码 PNG（需 tauri feature "image-png"）。
        let tray_icon = tauri::image::Image::from_bytes(TRAY_PNG)
            .map_err(|e| log::error!("poll_loop: embedded tray.png decode failed: {e}"))
            .ok();
        // urgent_count 防抖：仅在必须响应计数（权限+回答）变化时重画 badge
        let mut last_urgent_count: usize = 0;
        loop {
            let sessions = collector::collect_sessions();
            let merged = reducer::reduce(sessions);

            // derived snoozed：每轮基于 SnoozeMap 算，随 Session emit。
            // apply_snoozed 内部锁一次 guard 共用（避免每 session 各 lock 一次）。
            let snoozed_map = handle.try_state::<Mutex<snoozed::SnoozeMap>>();
            let derived: Vec<models::Session> = apply_snoozed(&merged, snoozed_map.as_deref());

            // reduce 之后、hash 之前：每轮都 observe，让 notifier 维护状态机
            let to_notify = notifier.observe(&derived);
            // 通知开关：prefs.notify=false 时静默（emit/tray badge 不受影响，只压通知）。
            let notify_on = handle
                .try_state::<Mutex<prefs::Prefs>>()
                .and_then(|s| s.lock().ok().map(|p| p.notify))
                .unwrap_or(true);
            if notify_on {
                for (name, status) in to_notify {
                    let status_zh = match status {
                        models::Status::NeedsPermission => "等待权限确认",
                        models::Status::WaitingForReply => "等待你回答",
                        models::Status::WaitingForInput => "等待输入",
                        _ => "需要关注",
                    };
                    notify::send_notification(
                        &handle,
                        "cc-view",
                        &format!("{}：{}", name, status_zh),
                    );
                }
            }

            // 聚合计数（一轮遍历）：
            //   perm   = 等权限（硬阻塞，工具待批准）
            //   reply  = 等回答（过程中提问，必须回答才能继续）
            //   idle   = 等输入（任务完成，等下一条指令）
            //   working 不排 snoozed（工作就是工作；搁置只压"等我"计数）
            // perm/reply/idle 排除 snoozed：搁置 = "暂时不管"，不催促（与 notify 语义一致）。
            let mut perm = 0usize;
            let mut reply = 0usize;
            let mut idle = 0usize;
            let mut working = 0usize;
            for s in &derived {
                if !s.alive {
                    continue;
                }
                match s.status {
                    models::Status::Working => working += 1,
                    _ if s.snoozed => {}
                    models::Status::NeedsPermission => perm += 1,
                    models::Status::WaitingForReply => reply += 1,
                    models::Status::WaitingForInput => idle += 1,
                    _ => {}
                }
            }
            // urgent = 必须立刻响应（权限 + 回答）→ 驱动 tray badge 红圆数字
            let urgent = perm + reply;

            if let Some(tray) = handle.tray_by_id("main") {
                // tooltip：urgent>0 时按权限/回答/等我/工作分段（仅显示 >0 的段）；
                //          否则 idle>0 → "等我·工作"；再否则 → "工作"。
                let tip = if urgent > 0 {
                    let mut parts: Vec<String> = Vec::new();
                    if perm > 0 {
                        parts.push(format!("{} 等权限", perm));
                    }
                    if reply > 0 {
                        parts.push(format!("{} 等回答", reply));
                    }
                    if idle > 0 {
                        parts.push(format!("{} 等我", idle));
                    }
                    parts.push(format!("{} 工作", working));
                    parts.join(" · ")
                } else if idle > 0 {
                    format!("{} 等我 · {} 工作", idle, working)
                } else {
                    format!("{} 工作", working)
                };
                // set_tooltip 走 main-thread IPC 但开销极小，每轮更新无妨
                let _ = tray.set_tooltip(Some(tip));

                // tray icon 二态：urgent>0 → badge（红圆数字 = 权限+回答总数，template=false）；
                //                 否则    → 单色剪影 + template=true（等输入也白色，用户偏好）。
                // 仅 urgent 计数变化时 set_icon（避免每轮 main-thread IPC 重绘）。
                if urgent != last_urgent_count {
                    last_urgent_count = urgent;
                    let (icon, as_template) = if urgent > 0 {
                        // badge 合成：基于单色剪影底图画红圆数字，template=false 才能显出红色。
                        (
                            tray_icon.as_ref().map(|img| badge::draw_badge(img, urgent)),
                            false,
                        )
                    } else {
                        (tray_icon.clone(), true)
                    };
                    if let Some(img) = icon {
                        let _ = tray.set_icon_with_as_template(Some(img), as_template);
                    }
                }
            }

            // 每轮刷新 sessions 缓存，供 focus_session command 查询 host。
            // cache 每轮更新，但 emit 仍受 hash 去重控制——稳定期也拿到最新 host。
            if let Some(cache) = handle.try_state::<Mutex<Vec<models::Session>>>() {
                if let Ok(mut c) = cache.lock() {
                    *c = derived.clone();
                } else {
                    log::warn!("poll_loop: sessions cache lock poisoned");
                }
            }

            let h = hash_sessions(&derived);
            if h != last_hash {
                last_hash = h;
                let _ = handle.emit("sessions", &derived);
            }
            // 间隔由偏好 AtomicU64 控制（默认 3，可 1-30）；无 state 则兜底 3。
            let secs = handle
                .try_state::<std::sync::atomic::AtomicU64>()
                .map(|a| a.load(std::sync::atomic::Ordering::Relaxed))
                .unwrap_or(3)
                .max(1);
            std::thread::sleep(Duration::from_secs(secs));
        }
    });
}

// --- Tauri commands：隐藏/取消隐藏/查询隐藏列表 ---
// lock 失败（poisoned）时 eprintln 提示 + 静默跳过——不崩溃前端调用（fail fast 可见性）。

/// 把会话加入隐藏列表并持久化。
#[tauri::command]
fn archive_session(state: tauri::State<'_, Mutex<archived::ArchivedList>>, id: String) {
    match state.lock() {
        Ok(mut h) => {
            h.add(&id);
            h.save();
        }
        Err(_) => log::warn!("archive_session: archived state lock poisoned"),
    }
}

/// 从隐藏列表移除会话并持久化。
#[tauri::command]
fn unarchive_session(state: tauri::State<'_, Mutex<archived::ArchivedList>>, id: String) {
    match state.lock() {
        Ok(mut h) => {
            h.remove(&id);
            h.save();
        }
        Err(_) => log::warn!("unarchive_session: archived state lock poisoned"),
    }
}

/// 返回当前隐藏会话 id 列表（前端据此 filter）。
#[tauri::command]
fn list_archived(state: tauri::State<'_, Mutex<archived::ArchivedList>>) -> Vec<String> {
    state.lock().map(|h| h.to_vec()).unwrap_or_else(|_| {
        log::warn!("list_archived: archived state lock poisoned");
        vec![]
    })
}

// --- Tauri commands：搁置/取消搁置/查询搁置表 ---
// 镜像 archive_session/unarchive_session/list_archived，但存时间戳（is_effectively_snoozed 需要）。

/// 标记会话搁置（记当前时间戳），持久化。前端乐观更新后由 poll_loop 对齐。
#[tauri::command]
fn snooze_session(state: tauri::State<'_, Mutex<snoozed::SnoozeMap>>, id: String) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    match state.lock() {
        Ok(mut m) => {
            m.add(&id, now_ms);
            m.save();
        }
        Err(_) => log::warn!("snooze_session: snoozed state lock poisoned"),
    }
}

/// 取消搁置（手动恢复），持久化。
#[tauri::command]
fn unsnooze_session(state: tauri::State<'_, Mutex<snoozed::SnoozeMap>>, id: String) {
    match state.lock() {
        Ok(mut m) => {
            m.remove(&id);
            m.save();
        }
        Err(_) => log::warn!("unsnooze_session: snoozed state lock poisoned"),
    }
}

/// 返回搁置表 {id: snoozedAt}（前端用于乐观更新/调试）。
#[tauri::command]
fn list_snoozed(
    state: tauri::State<'_, Mutex<snoozed::SnoozeMap>>,
) -> std::collections::HashMap<String, i64> {
    state.lock().map(|m| m.to_map()).unwrap_or_else(|_| {
        log::warn!("list_snoozed: snoozed state lock poisoned");
        std::collections::HashMap::new()
    })
}

/// 立即采集并返回当前所有会话（供前端打开时拉初始数据 / 手动刷新）。
/// 不依赖 poll loop 的 3s 节拍与 hash 去重——调用即拿实时结果。
/// 同时算 derived snoozed，避免首次打开到 poll_loop emit 之间显示不准。
/// 同时刷新 cache，让 focus_session 也能用到最新 host。
#[tauri::command]
fn get_sessions(
    cache: tauri::State<'_, Mutex<Vec<models::Session>>>,
    snoozed: tauri::State<'_, Mutex<snoozed::SnoozeMap>>,
) -> Vec<models::Session> {
    let merged = reducer::reduce(collector::collect_sessions());
    let derived = apply_snoozed(&merged, Some(snoozed.inner()));
    match cache.lock() {
        Ok(mut c) => *c = derived.clone(),
        Err(_) => log::warn!("get_sessions: cache lock poisoned"),
    }
    derived
}

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

/// 按 session id 激活对应终端。未授辅助功能权限时弹系统授权窗 + 返回 Err("accessibility")
/// 让前端提示。仍尝试 activate（open -a 对非全屏终端生效）；click Dock 切全屏 Space 需要权限。
#[tauri::command]
fn focus_session(
    id: String,
    cache: tauri::State<'_, Mutex<Vec<models::Session>>>,
) -> Result<(), String> {
    let need_perm = !focus::ax_trusted(false);
    if need_perm {
        focus::ax_trusted(true); // 弹系统授权窗（首次进列表，重复调用安全）
    }
    let (host, tty, cwd) = match cache.lock() {
        Ok(sessions) => sessions
            .iter()
            .find(|s| s.id == id)
            .map(|s| {
                (
                    s.focus_hint.host.clone(),
                    s.focus_hint.tty.clone(),
                    s.cwd.clone(),
                )
            })
            .ok_or_else(|| format!("focus_session: session {} not in cache", id)),
        Err(_) => Err("focus_session: sessions cache lock poisoned".to_string()),
    }?;
    focus::activate_host(&host, &tty, &cwd);
    if need_perm {
        Err("accessibility".into())
    } else {
        Ok(())
    }
}

// --- overlay pin（图钉：失焦是否自动收起）command ---
// pin 状态由 app State<Mutex<bool>> 持有，失焦双机制读它判断要不要 hide。
// set 时同步持久化（保留磁盘 x,y），开机/重启按记忆恢复。

/// 读取 overlay 是否钉住（失焦不收起）。State 初始从磁盘恢复，无记录 false。
#[tauri::command]
fn get_overlay_pinned(state: tauri::State<'_, Mutex<bool>>) -> bool {
    *state.lock().unwrap()
}

/// 切换 overlay 钉住状态：更新 State + 持久化（保留现有 x,y）。
/// 三档 fallback 避免新装用户无位置文件时落 (0,0)：
///   1) 读窗口当前 outer_position（pin 通常在呼出可见时操作，最准）
///   2) 窗口不可见/拿不到 → 回退磁盘记录（仅替换 pinned，保留 x,y）
///   3) 无窗口 + 无文件 → 不写（等 Moved 事件或下次 show 自然落盘）
#[tauri::command]
fn set_overlay_pinned(pinned: bool, state: tauri::State<'_, Mutex<bool>>, app: tauri::AppHandle) {
    *state.lock().unwrap() = pinned;
    // 1) 优先读窗口当前位置（pin 通常在呼出可见时操作）。
    if let Some(w) = app.get_webview_window("overlay") {
        if let Ok(pos) = w.outer_position() {
            overlay_position::OverlayPosition::save_all(pos.x, pos.y, pinned);
            return;
        }
    }
    // 2) overlay 不可见时：仅当磁盘已有记录才更新（保留 x,y，只换 pinned）。
    if let Some(p) = overlay_position::OverlayPosition::load() {
        overlay_position::OverlayPosition::save_all(p.x, p.y, pinned);
    }
    // 3) 无窗口 + 无文件：不写（等 Moved 事件或下次 show 时自然落盘）。
}

// --- 偏好设置 commands ---
// notify/shortcut/interval 存 Mutex<Prefs>，改后立即 save。autostart 走插件自管。
// 校验遵循 fail fast：非法 shortcut/interval 返回 Err 给前端。

#[tauri::command]
fn get_prefs(state: tauri::State<'_, Mutex<prefs::Prefs>>) -> prefs::Prefs {
    state.lock().map(|p| p.clone()).unwrap_or_default()
}

#[tauri::command]
fn set_notify(notify: bool, state: tauri::State<'_, Mutex<prefs::Prefs>>) {
    if let Ok(mut p) = state.lock() {
        p.notify = notify;
        p.save();
    }
}

/// 开/关开机自启动。enable=true→enable()，false→disable()。插件错误转 String 返回前端。
#[tauri::command]
fn toggle_autostart(app: tauri::AppHandle, enable: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    if enable {
        mgr.enable().map_err(|e| e.to_string())?
    } else {
        mgr.disable().map_err(|e| e.to_string())?
    }
    Ok(())
}

#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// 切换全局快捷键：unregister_all → 按新值 register（off 则不注册）→ 存 prefs。
/// 失败（解析/注册）返回 Err，不落库。
#[tauri::command]
fn set_shortcut(
    shortcut: String,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if !prefs::Prefs::is_valid_shortcut(&shortcut) {
        return Err(format!("invalid shortcut: {}", shortcut));
    }
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())?;
    // cmd+comma 固定（开偏好）——unregister_all 会清掉，每次 re-register。
    app.global_shortcut()
        .register("cmd+comma")
        .map_err(|e| e.to_string())?;
    if shortcut != "off" {
        // register 接受 TryInto<ShortcutWrapper>；&str 直接满足，内部 parse。
        app.global_shortcut()
            .register(shortcut.as_str())
            .map_err(|e| e.to_string())?;
    }
    if let Ok(mut p) = state.lock() {
        p.shortcut = shortcut;
        p.save();
    }
    Ok(())
}

/// 设置轮询间隔（1-30 秒）：更新 AtomicU64（poll_loop 读）+ 存 prefs。
#[tauri::command]
fn set_interval(
    seconds: u64,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    interval: tauri::State<'_, std::sync::atomic::AtomicU64>,
) -> Result<(), String> {
    if !(1..=30).contains(&seconds) {
        return Err(format!("interval must be 1-30, got {}", seconds));
    }
    interval.store(seconds, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut p) = state.lock() {
        p.poll_interval = seconds;
        p.save();
    }
    Ok(())
}

/// 设置常驻布局（B 精简 / A 极简）：存 prefs + emit prefs_changed 让 overlay 窗口响应。
#[tauri::command]
fn set_resident_layout(
    layout: prefs::ResidentLayout,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) {
    if let Ok(mut p) = state.lock() {
        p.resident_layout = layout;
        p.save();
    }
    let _ = app.emit("prefs_changed", ());
}

/// 切换常驻模式是否显示搁置的会话：存 prefs + emit prefs_changed。
#[tauri::command]
fn set_resident_show_snoozed(
    show: bool,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) {
    if let Ok(mut p) = state.lock() {
        p.resident_show_snoozed = show;
        p.save();
    }
    let _ = app.emit("prefs_changed", ());
}

/// 切换常驻模式是否显示闲置（等输入超时）的会话：存 prefs + emit prefs_changed。
#[tauri::command]
fn set_resident_show_idle(
    show: bool,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) {
    if let Ok(mut p) = state.lock() {
        p.resident_show_idle = show;
        p.save();
    }
    let _ = app.emit("prefs_changed", ());
}

/// 切换是否显示已归档会话（全局：面板 toggle 写，面板+常驻共享读）：存 prefs + emit prefs_changed。
#[tauri::command]
fn set_show_archived(
    show: bool,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) {
    if let Ok(mut p) = state.lock() {
        p.show_archived = show;
        p.save();
    }
    let _ = app.emit("prefs_changed", ());
}

/// 设置常驻背景透明度（0–100）：校验失败返回 Err（fail fast），合法则存 prefs + emit。
#[tauri::command]
fn set_resident_opacity(
    opacity: u8,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if !prefs::Prefs::is_valid_opacity(opacity) {
        return Err(format!("opacity must be 0-100, got {}", opacity));
    }
    if let Ok(mut p) = state.lock() {
        p.resident_opacity = opacity;
        p.save();
    }
    let _ = app.emit("prefs_changed", ());
    Ok(())
}

/// 设置常驻面板宽度（140–480）：存 prefs + resident 模式下 setFrame 更新几何（右边锚定）。
/// panel 模式只存 prefs 不动窗口（panel 固定 560×420）。越界返回 Err（fail fast）。
#[tauri::command]
fn set_resident_width(
    width: f64,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if !(140.0..=480.0).contains(&width) {
        return Err(format!("width must be 140-480, got {}", width));
    }
    // 一次 lock 取 mode + 存 width（避免两次 lock 同一 Mutex）
    let mode = if let Ok(mut p) = state.lock() {
        let m = p.mode;
        p.resident_width = Some(width);
        p.save();
        m
    } else {
        prefs::OverlayMode::Resident
    };
    #[cfg(target_os = "macos")]
    {
        if mode == prefs::OverlayMode::Resident {
            set_resident_window_width(&app, width);
        }
    }
    let _ = app.emit("prefs_changed", ());
    Ok(())
}

/// 设置外观主题（light/dark）：存 prefs + 强制 overlay/prefs 窗口 appearance + emit prefs_changed。
/// theme 参数为 prefs::Theme，非法值（非 light/dark）由 serde deserialize 失败自动返回 Err（fail fast）。
#[tauri::command]
fn set_theme(
    theme: prefs::Theme,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if let Ok(mut p) = state.lock() {
        p.theme = theme;
        p.save();
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(w) = app.get_webview_window("overlay") {
            apply_theme_to_window(&w, theme);
        }
        if let Some(w) = app.get_webview_window("prefs") {
            apply_theme_to_window(&w, theme);
        }
    }
    let _ = app.emit("prefs_changed", ());
    Ok(())
}

/// 设置 token 量单位（km/wan）：存 prefs + emit prefs_changed（前端 tokenUnit 响应）。
#[tauri::command]
fn set_token_unit(
    unit: prefs::TokenUnit,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) {
    if let Ok(mut p) = state.lock() {
        p.token_unit = unit;
        p.save();
    }
    let _ = app.emit("prefs_changed", ());
}

/// 设置是否显示终端 app 名：存 prefs + emit prefs_changed。
#[tauri::command]
fn set_show_host(
    show: bool,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) {
    if let Ok(mut p) = state.lock() {
        p.show_host = show;
        p.save();
    }
    let _ = app.emit("prefs_changed", ());
}

/// 设置是否显示 token 用量：存 prefs + emit prefs_changed。
#[tauri::command]
fn set_show_tokens(
    show: bool,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) {
    if let Ok(mut p) = state.lock() {
        p.show_tokens = show;
        p.save();
    }
    let _ = app.emit("prefs_changed", ());
}

/// 设置是否显示操作按钮：存 prefs + emit prefs_changed。
#[tauri::command]
fn set_show_actions(
    show: bool,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) {
    if let Ok(mut p) = state.lock() {
        p.show_actions = show;
        p.save();
    }
    let _ = app.emit("prefs_changed", ());
}

/// 切换 overlay 模式（panel / resident）：存 prefs + resize 窗口 + 重新定位 + emit mode_changed
/// 让前端（overlay 窗口）切换视图；同时 emit prefs_changed 让 ResidentView 重读配置。
/// prefs 窗口的 set_mode 调用同样 emit，overlay 会响应。
#[tauri::command]
fn set_mode(
    mode: prefs::OverlayMode,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // 存 mode + emit；不立即动画——前端 mode_changed 先显示 spinner 100ms（窗口未动），
    // 再 invoke do_animate 触发窗口动画，避免 spinner 和动画同时出现、spinner 在变窗口里闪。
    if let Ok(mut p) = state.lock() {
        p.mode = mode;
        p.save();
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    LAST_MODE_CHANGE_MS.store(now_ms, std::sync::atomic::Ordering::Relaxed);
    let _ = app.emit("mode_changed", mode);
    let _ = app.emit("prefs_changed", ());
    Ok(())
}

/// 前端 spinner 就位后调（mode_changed 后 100ms）：按当前 mode 启动窗口缩放动画。
#[tauri::command]
fn do_animate(state: tauri::State<'_, Mutex<prefs::Prefs>>, app: tauri::AppHandle) {
    let (mode, layout) = state
        .lock()
        .map(|p| (p.mode, p.resident_layout))
        .unwrap_or((prefs::OverlayMode::Resident, prefs::ResidentLayout::B));
    apply_mode_window(&app, mode, layout);
}

/// 校正常驻窗口高度为内容实际高度（前端量得渲染高度后调用）。
/// 仅当前 mode==resident 时生效——避免面板模式被误改。宽度按当前 resident_layout。
#[tauri::command]
fn set_resident_height(
    height: f64,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) {
    // 模式切换动画进行中：跳过高度校正，避免与 animate_window_to 的 set_size 冲突；
    // 动画结束后窗口尺寸稳定，前端 ResizeObserver 会重新触发 syncHeight。
    if ANIMATING.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let (mode, layout, resident_width) = state
        .lock()
        .map(|p| (p.mode, p.resident_layout, p.resident_width))
        .unwrap_or((prefs::OverlayMode::Resident, prefs::ResidentLayout::B, None));
    if mode != prefs::OverlayMode::Resident {
        return;
    }
    let width = resident_width.unwrap_or_else(|| resident_layout_width(layout));
    let Some(w) = app.get_webview_window("overlay") else {
        return;
    };
    let _ = w.set_size(tauri::LogicalSize::new(width, height));
}

/// 让 overlay 窗口加入所有 Space（含全屏 app 独占 Space）。
/// macOS 全屏应用占据独立 Space，普通 NSWindow 默认不跨 Space → 弹到桌面 Space 用户看不到。
/// Spotlight/Raycast 解法：设 NSWindowCollectionBehavior 的两个 flag：
///   - CanJoinAllSpaces    (1 << 0)   跨所有 Space 显示
///   - FullScreenAuxiliary (1 << 8)   作为全屏辅助浮层，盖在全屏 app 内容之上
/// 合计 = 1 | 256 = 257。
#[cfg(target_os = "macos")]
fn join_all_spaces(w: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(ptr) = w.ns_window() else {
        log::warn!("join_all_spaces: ns_window unavailable, overlay will not cross spaces");
        return;
    };
    let obj = ptr as *mut AnyObject;
    // CanJoinAllSpaces (1<<0) | FullScreenAuxiliary (1<<8)
    let behavior: objc2::ffi::NSUInteger = (1 << 0) | (1 << 8);
    // NSPopUpMenuWindowLevel = 101（Spotlight/Raycast 同款层级）。
    // 高 level 让 macOS 把窗口当系统级浮层，配合 canJoinAllSpaces 才能真正跨 Space（含全屏）。
    let level: objc2::ffi::NSInteger = 101;
    unsafe {
        let _: () = msg_send![obj, setCollectionBehavior: behavior];
        let _: () = msg_send![obj, setLevel: level];
        // 读回确认（诊断用）。
        let val: objc2::ffi::NSUInteger = msg_send![obj, collectionBehavior];
        log::debug!(
            "overlay collectionBehavior = {} (expect 257), level = 101",
            val
        );
    }
}

/// 切换 app activation policy：0=regular（有 dock），1=accessory（无 dock）。
/// cc-view 平时 accessory（LSUIElement）；打开偏好设置需 regular 给用户 app 入口。
#[cfg(target_os = "macos")]
fn set_activation_policy(policy: i64) {
    use objc2::{class, msg_send, runtime::AnyObject};
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setActivationPolicy: policy as objc2::ffi::NSInteger];
    }
}

/// 强制窗口外观为 light/dark（NSAppearance），让 vibrancy 跟 theme 走而非系统。
/// light → NSAppearanceNameAqua，dark → NSAppearanceNameDarkAqua。
/// 用 NSString stringWithUTF8String: 把 C 字符串转 NSString（项目无 objc2-foundation，不加依赖）。
#[cfg(target_os = "macos")]
fn apply_theme_to_window(w: &tauri::WebviewWindow, theme: prefs::Theme) {
    use objc2::{class, msg_send, runtime::AnyObject};
    use std::ffi::CString;
    let Ok(ptr) = w.ns_window() else { return };
    let ns_window = ptr as *mut AnyObject;
    let name = match theme {
        prefs::Theme::Light => "NSAppearanceNameAqua",
        prefs::Theme::Dark => "NSAppearanceNameDarkAqua",
    };
    let Ok(cstr) = CString::new(name) else { return };
    unsafe {
        let nsstr: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: cstr.as_ptr()];
        let appearance: *mut AnyObject = msg_send![class!(NSAppearance), appearanceNamed: nsstr];
        let _: () = msg_send![ns_window, setAppearance: appearance];
    }
}

/// 把 overlay 的 NSWindow isa swizzle 成 NSPanel（Spotlight/Raycast 做法）。
/// NSPanel + nonActivatingPanel 能在不激活 app 的情况下 become key 接受输入，
/// 从而不触发 Space 归属/切换——这是普通 NSWindow 跨全屏 Space 的唯一可靠解法。
/// NSPanel 是 NSWindow 子类且不加 ivar，object_setClass 安全；Tauri 的
/// show/hide/focus/vibrancy 调的都是 NSWindow 方法，swizzle 后仍正常。
#[cfg(target_os = "macos")]
/// NSPanel canBecomeKeyWindow 强制返回 YES：borderless（Tauri 无标题栏）window 默认 false，
/// 导致 makeKey 不成 key、搜索框无法 input。titled styleMask 与 NSPanel swizzle 冲突 panic，
/// 故用 method swizzle 直接替换 NSPanel 的 canBecomeKeyWindow 实现（cc-view 只一个 NSPanel，全局安全）。
#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn panel_can_become_key(
    _this: *mut objc2::runtime::AnyObject,
    _cmd: objc2::runtime::Sel,
) -> objc2::runtime::Bool {
    objc2::runtime::Bool::YES
}

fn make_panel(w: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(ptr) = w.ns_window() else {
        log::warn!("make_panel: ns_window unavailable");
        return;
    };
    let obj = ptr as *mut AnyObject;
    unsafe {
        // isa swizzle: NSWindow → NSPanel
        // ffi::object_setClass 签名: (*mut AnyObject, *const AnyClass) -> *const AnyClass
        let panel = objc2::class!(NSPanel);
        objc2::ffi::object_setClass(obj, panel as *const _);
        // mask：nonActivatingPanel(1<<7) + titled(1<<0) + fullSizeContentView(1<<13)。
        // 关键：titled 让 canBecomeKeyWindow=true——borderless（Tauri decorations:false）默认 false，
        // 导致 makeKey 不成 key、搜索框无法 input。fullSizeContentView + titlebar transparent +
        // title hidden 视觉保持无标题栏。
        let mask: objc2::ffi::NSUInteger = msg_send![obj, styleMask];
        let _: () = msg_send![obj, setStyleMask: mask | (1 << 7)];
        // 强制替换 NSPanel 的 canBecomeKeyWindow 返回 true：borderless（Tauri 无标题栏）默认 false
        // → makeKey 不成 key → 搜索框无法 input。titled styleMask 与 swizzle 冲突 panic，故走 method swizzle。
        let ns_panel: *mut objc2::runtime::AnyClass = objc2::class!(NSPanel) as *const _ as *mut _;
        let fn_ptr: unsafe extern "C-unwind" fn(
            *mut objc2::runtime::AnyObject,
            objc2::runtime::Sel,
        ) -> objc2::runtime::Bool = panel_can_become_key;
        let imp_fn: unsafe extern "C-unwind" fn() = std::mem::transmute(fn_ptr);
        objc2::ffi::class_replaceMethod(
            ns_panel,
            objc2::sel!(canBecomeKeyWindow),
            imp_fn,
            b"B@:\0".as_ptr() as *const std::os::raw::c_char,
        );
        // becomesKeyOnlyIfNeeded = false：nonActivatingPanel 默认 true（只在 hit view 的
        // needsPanelToBecomeKey 返回 true 时才 become key），WKWebView 不触发 → makeKey 不成 key。
        let _: () = msg_send![obj, setBecomesKeyOnlyIfNeeded: false];
        log::debug!("overlay swizzled to NSPanel (canBecomeKeyWindow forced true)");
    }
}

/// 返回当前前台 app 的 bundle id（NSWorkspace.sharedWorkspace.frontmostApplication.bundleIdentifier）。
/// 用于 overlay 失焦检测：NSPanel nonActivatingPanel 不触发 Focused(false)，
/// 改查 frontmost app 是否变化（变了 = 用户切到别的 app）。cc-view 自身是 accessory app
/// （LSUIElement）从不是 frontmost，所以基准值是用户呼出时所在的 app。
#[cfg(target_os = "macos")]
fn frontmost_bundle_id() -> Option<String> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::ffi::CStr;

    unsafe {
        // [NSWorkspace sharedWorkspace]
        let ws: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
        // .frontmostApplication（NSRunningApplication?，刚启动无前台 app 时可能为 nil）
        let app: Option<Retained<AnyObject>> = msg_send![&ws, frontmostApplication];
        let app = app?;
        // .bundleIdentifier（NSString?，可能为 nil）
        let bid: Option<Retained<AnyObject>> = msg_send![&app, bundleIdentifier];
        let bid = bid?;
        // [NSString UTF8String] → *const c_char（null-terminated UTF-8，NSString 生命周期与 bid 绑定）
        let utf8: *const std::os::raw::c_char = msg_send![&bid, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}

/// 呼出 overlay：恢复/居中位置 → show → makeKey → 启动失焦轮询。
/// 快捷键 ⌥Space 与 tray 菜单「显示面板」共用。
fn show_overlay(app: &tauri::AppHandle) {
    let Some(w) = app.get_webview_window("overlay") else {
        return;
    };
    // 已可见直接 return：避免 tray 菜单「显示面板」重复点击 spawn 第二个 frontmost 轮询线程。
    // （⌥Space handler 已有自己的 is_visible 守卫，此处不影响它。）
    if w.is_visible().unwrap_or(false) {
        return;
    }
    // show 前设 collectionBehavior + level，否则被钉在桌面 Space。
    #[cfg(target_os = "macos")]
    join_all_spaces(&w);
    if let Some(pos) = overlay_position::OverlayPosition::load() {
        // 校验坐标在屏幕内——防坏坐标恢复到屏外（与 setup 一致）。
        let in_bounds = w
            .current_monitor()
            .ok()
            .flatten()
            .map(|m| {
                let s = m.size();
                let p = m.position();
                pos_in_rect(pos.x, pos.y, p.x, p.y, s.width as i32, s.height as i32)
            })
            .unwrap_or(true);
        if in_bounds {
            let _ = w.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
        } else {
            let _ = w.center(); // 坏坐标 → center 兜底
        }
    } else {
        let _ = w.center();
    }
    let _ = w.show();
    // window 必须 become key 才能让搜索框 focus/输入、顶栏拖动生效。canBecomeKeyWindow 已由
    // make_panel 的 method swizzle 强制 true（borderless 默认 false）。makeKeyAndOrderFront 成为
    // key；activateIgnoringOtherApps 激活 app（WKWebView input 需 app active）。
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::{class, msg_send, runtime::AnyObject};
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, activateIgnoringOtherApps: true];
        if let Ok(p) = w.ns_window() {
            let nil: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![p as *mut AnyObject, makeKeyAndOrderFront: nil];
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = w.set_focus();
    #[cfg(target_os = "macos")]
    join_all_spaces(&w);

    // 失焦轮询（钉住时按 pin 跳过 hide，见轮询内判断）
    #[cfg(target_os = "macos")]
    {
        let app_handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let stable_front = frontmost_bundle_id();
            loop {
                // 间隔按 mode：panel 200ms 抓快速切走；resident 2s（只检测窗口关 / mode 切回 panel）。
                // resident 不查 frontmost（省 objc 链调用），panel 才查——省 ~90% 轮询开销。
                let mode = app_handle
                    .state::<Mutex<prefs::Prefs>>()
                    .lock()
                    .map(|p| p.mode)
                    .unwrap_or(prefs::OverlayMode::Resident);
                let gap = if mode == prefs::OverlayMode::Resident {
                    2000
                } else {
                    200
                };
                std::thread::sleep(std::time::Duration::from_millis(gap));
                let Some(win) = app_handle.get_webview_window("overlay") else {
                    break;
                };
                if !win.is_visible().unwrap_or(false) {
                    break;
                }
                // resident：always-pinned，不查 frontmost 不 hide，下一轮再判 mode。
                if mode == prefs::OverlayMode::Resident {
                    continue;
                }
                // panel：查 frontmost，切走了且未 pin 则 hide。
                let current_front = frontmost_bundle_id();
                if current_front != stable_front {
                    let pinned = app_handle
                        .state::<Mutex<bool>>()
                        .lock()
                        .map(|g| *g)
                        .unwrap_or(false);
                    if !pinned {
                        let _ = win.hide();
                        break;
                    }
                }
            }
        });
    }
}

/// 打开偏好设置窗口：转 regular（dock 出现）→ show → focus。
/// accessory app 默认无 dock，点偏好设置时需切 regular 提供 app 入口。
fn open_prefs(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    set_activation_policy(0); // NSApplicationActivationPolicyRegular
    if let Some(w) = app.get_webview_window("prefs") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 日志：默认 warn（release 静默诊断），RUST_LOG=debug 开调试。
    // try_init 容忍 tauri 自身可能已初始化 logger，不 panic。
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();
    let loaded_prefs = prefs::Prefs::load();
    let poll_secs = loaded_prefs.poll_interval;
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(Mutex::new(archived::ArchivedList::load()))
        .manage(Mutex::new(snoozed::SnoozeMap::load()))
        .manage(Mutex::new(Vec::<models::Session>::new()))
        .manage(Mutex::new(
            overlay_position::OverlayPosition::load()
                .map(|p| p.pinned)
                .unwrap_or(false),
        ))
        .manage(std::sync::atomic::AtomicU64::new(poll_secs))
        .manage(Mutex::new(loaded_prefs))
        .invoke_handler(tauri::generate_handler![
            archive_session,
            unarchive_session,
            list_archived,
            focus_session,
            get_sessions,
            get_session_detail,
            get_overlay_pinned,
            set_overlay_pinned,
            snooze_session,
            unsnooze_session,
            list_snoozed,
            get_prefs,
            set_notify,
            toggle_autostart,
            get_autostart,
            set_shortcut,
            set_interval,
            set_resident_layout,
            set_resident_show_snoozed,
            set_resident_show_idle,
            set_show_archived,
            set_resident_opacity,
            set_resident_width,
            set_theme,
            set_token_unit,
            set_show_host,
            set_show_tokens,
            set_show_actions,
            set_mode,
            do_animate,
            set_resident_height
        ])
        .setup(|app| {
            // Tauri 默认 activation policy = Regular（有 dock），覆盖 Info.plist LSUIElement。
            // cc-view 平时 accessory（无 dock）——启动显式 set Accessory；打开偏好设置时切 Regular。
            let _ = app
                .handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory);

            // 启动按当前 theme 强制窗口 appearance（让 vibrancy 跟 theme 而非系统）。
            let startup_theme = app
                .state::<Mutex<prefs::Prefs>>()
                .lock()
                .map(|p| p.theme)
                .unwrap_or(prefs::Theme::Light);

            // 通知权限请求块——保留以兼容未来插件版本。
            // 上游限制（tauri-plugin-notification v2.3.3）：桌面端 request_permission() /
            // permission_state() 是 no-op stub，总返回 Ok(Granted)，故下方 matches!(Prompt)
            // 永远 false、request_permission 永不执行，当前无应用内弹窗。macOS 通过旧版
            // NSUserNotificationCenter 按系统设置自动处理通知权限。若未来插件实现真实桌面权限
            // 检查，此块即自动生效（首次 Prompt 时请求，Granted/Denied 后不再烦扰）。
            use tauri::plugin::PermissionState;
            use tauri_plugin_notification::NotificationExt;
            let notif = app.notification();
            if matches!(notif.permission_state(), Ok(PermissionState::Prompt)) {
                let _ = notif.request_permission();
            }

            // overlay 窗口：失焦自动 hide（Alfred/uTools 行为——点别处就收起）。
            // on_window_event 闭包签名是 Fn(&WindowEvent)（单参），拿不到 window 引用——
            // 外层 clone WebviewWindow（Tauri 2 派生 Clone，是廉价 handle 非拥有资源）
            // 再 move 进闭包，失焦时调 hide()。仅 overlay 有此行为。
            // vibrancy material：UnderWindowBackground（中性偏深）——浅色下不如 Menu 刺白、深色下更深，
            // 一次解决"透明度 0 露出突兀白块"与"深色文字发糊"（Menu 偏亮、深色 tint 压不住）。
            // 配合 set_theme 强制的 NSAppearance，浅/深行为可控。EffectState::Active 失焦仍保持毛玻璃；radius 12 观感更柔。
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.set_effects(
                    EffectsBuilder::new()
                        .effect(Effect::UnderWindowBackground)
                        .state(EffectState::Active)
                        .radius(12.)
                        .build(),
                );

                #[cfg(target_os = "macos")]
                apply_theme_to_window(&overlay, startup_theme);

                // 跨 Space / 全屏可见：overlay 需在全屏 app 下也能弹出（Spotlight/Raycast 行为）
                #[cfg(target_os = "macos")]
                join_all_spaces(&overlay);

                // isa swizzle NSWindow → NSPanel（一次性）：
                // nonActivatingPanel 才能真正跨全屏 Space（become key 不激活 app → 不切 Space）。
                // 必须在 vibriosity set_effects 之后、窗口 show 之前调。
                #[cfg(target_os = "macos")]
                make_panel(&overlay);

                // 恢复上次保存的 overlay 位置（vibrancy / swizzle 之后）。
                // 无记录时跳过——由呼出时的 center() 兜底。
                if let Some(pos) = overlay_position::OverlayPosition::load() {
                    // 校验坐标在屏幕内——防坏坐标（如宽度 bug 残留）恢复到屏外。
                    let in_bounds = overlay
                        .current_monitor()
                        .ok()
                        .flatten()
                        .map(|m| {
                            let s = m.size();
                            let p = m.position();
                            pos_in_rect(pos.x, pos.y, p.x, p.y, s.width as i32, s.height as i32)
                        })
                        .unwrap_or(true);
                    if in_bounds {
                        let _ = overlay.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
                    }
                    // 坏坐标：skip → 窗口保持 tauri.conf center 初始
                }

                // mode=resident 时按持久化宽度初始化窗口尺寸（panel 保持 560×420 初始）。
                let (startup_mode, startup_width, startup_layout) = app
                    .state::<Mutex<prefs::Prefs>>()
                    .lock()
                    .map(|p| (p.mode, p.resident_width, p.resident_layout))
                    .unwrap_or((prefs::OverlayMode::Resident, None, prefs::ResidentLayout::B));
                if startup_mode == prefs::OverlayMode::Resident {
                    let lw = startup_width.unwrap_or_else(|| resident_layout_width(startup_layout));
                    let _ = overlay.set_size(tauri::LogicalSize::new(lw, PANEL_H));
                }

                let w = overlay.clone();
                let app_handle = app.handle().clone();
                overlay.on_window_event(move |e| match e {
                    tauri::WindowEvent::Moved(p) => {
                        // debounce：更新待落盘坐标 + 时间戳；起单线程静止 300ms 后落盘。
                        if let Ok(mut g) = PENDING_MOVE_POS.lock() {
                            *g = Some((p.x, p.y));
                        }
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        LAST_MOVE_MS.store(now_ms, std::sync::atomic::Ordering::Relaxed);
                        // 仅当无 debounce 线程在跑时启动（单线程 trailing debounce）
                        if !DEBOUNCE_ACTIVE.swap(true, std::sync::atomic::Ordering::AcqRel) {
                            std::thread::spawn(|| loop {
                                std::thread::sleep(std::time::Duration::from_millis(300));
                                let last = LAST_MOVE_MS.load(std::sync::atomic::Ordering::Relaxed);
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0);
                                if now.saturating_sub(last) >= 300 {
                                    let pos =
                                        PENDING_MOVE_POS.lock().ok().and_then(|mut g| g.take());
                                    if let Some((x, y)) = pos {
                                        overlay_position::OverlayPosition::save(x, y);
                                    }
                                    DEBOUNCE_ACTIVE
                                        .store(false, std::sync::atomic::Ordering::Release);
                                    return;
                                }
                            });
                        }
                    }
                    tauri::WindowEvent::Focused(false) => {
                        // 常驻模式 = always-pinned（失焦不收起）；面板模式按 pin 决定。
                        let mode = app_handle
                            .state::<Mutex<prefs::Prefs>>()
                            .lock()
                            .map(|p| p.mode)
                            .unwrap_or(prefs::OverlayMode::Resident);
                        let pinned = app_handle
                            .state::<Mutex<bool>>()
                            .lock()
                            .map(|g| *g)
                            .unwrap_or(false);
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        let last_mode =
                            LAST_MODE_CHANGE_MS.load(std::sync::atomic::Ordering::Relaxed);
                        // mode 切换后 1.5s 内不 hide：动画期间窗口可能瞬间 resign key
                        let in_grace = now_ms.saturating_sub(last_mode) < 1500;
                        let will_hide =
                            mode != prefs::OverlayMode::Resident && !pinned && !in_grace;
                        log::debug!(
                            "overlay Focused(false): will_hide={} mode={:?} pinned={} in_grace={}",
                            will_hide,
                            mode,
                            pinned,
                            in_grace
                        );
                        if will_hide {
                            let _ = w.hide();
                        }
                    }
                    _ => {}
                });
            }

            // prefs 窗口：关闭即转回 accessory（dock 消失）+ hide（不销毁，下次复用）。
            if let Some(prefs_win) = app.get_webview_window("prefs") {
                #[cfg(target_os = "macos")]
                apply_theme_to_window(&prefs_win, startup_theme);

                let prefs_handle = app.handle().clone();
                prefs_win.on_window_event(move |e| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                        api.prevent_close();
                        #[cfg(target_os = "macos")]
                        set_activation_policy(1); // accessory
                        if let Some(w) = prefs_handle.get_webview_window("prefs") {
                            let _ = w.hide();
                        }
                    }
                });
            }

            // 构建 menubar 托盘菜单：版本号(只读) / 显示面板 / 偏好设置(占位) / 检查更新(占位) / 退出。
            let version = env!("CARGO_PKG_VERSION");
            let version_item = MenuItem::with_id(
                app.handle(),
                "version",
                &format!("cc-view {version}"),
                false,
                None::<&str>,
            )?;
            let sep1 = PredefinedMenuItem::separator(app.handle())?;
            let show_item =
                MenuItem::with_id(app.handle(), "show", "显示面板", true, None::<&str>)?;
            let sep2 = PredefinedMenuItem::separator(app.handle())?;
            let prefs_item =
                MenuItem::with_id(app.handle(), "prefs", "偏好设置…", true, None::<&str>)?;
            let update_item =
                MenuItem::with_id(app.handle(), "update", "检查更新…", true, None::<&str>)?;
            let sep3 = PredefinedMenuItem::separator(app.handle())?;
            let quit_item =
                MenuItem::with_id(app.handle(), "quit", "退出 cc-view", true, None::<&str>)?;
            let menu = Menu::with_items(
                app.handle(),
                &[
                    &version_item,
                    &sep1,
                    &show_item,
                    &sep2,
                    &prefs_item,
                    &update_item,
                    &sep3,
                    &quit_item,
                ],
            )?;

            // tray icon 已在 tauri.conf.json 声明（id="main"），取出附菜单。
            // 左键弹菜单（showMenuOnLeftClick: true）——不再 on_tray_icon_event toggle。
            let tray = app
                .tray_by_id("main")
                .ok_or_else(|| tauri::Error::AssetNotFound("tray icon 'main'".to_string()))?;
            tray.set_menu(Some(menu))?;

            // 菜单事件：show → 呼出 overlay；prefs → 打开偏好（转 regular）；quit → 退出。version 占位 no-op；update → 打开偏好。
            app.on_menu_event(|app, event| match event.id().as_ref() {
                "show" => show_overlay(app),
                "prefs" => open_prefs(app),
                "update" => open_prefs(app),
                "quit" => app.exit(0),
                _ => {}
            });

            // 快捷键按 prefs.shortcut 注册（默认 alt+space，可改/禁用）。
            // handler 不写死组合——对当前注册的任意快捷键都 toggle overlay。
            // 核对 v2.x：with_shortcuts 接受 [&str]，"cmd+alt+space"/"ctrl+space" 能解析。
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{Builder, Code, Modifiers, ShortcutState};
                let shortcut_str = app
                    .state::<Mutex<prefs::Prefs>>()
                    .lock()
                    .map(|p| p.shortcut.clone())
                    .unwrap_or_else(|_| "alt+space".into());
                // cmd+comma 固定注册（开偏好，VSCode/macOS 习惯）；overlay 快捷键按 prefs（可 off）。
                let mut shortcuts: Vec<&str> = vec!["cmd+comma"];
                if shortcut_str != "off" {
                    shortcuts.push(shortcut_str.as_str());
                }
                app.handle().plugin(
                    Builder::new()
                        .with_shortcuts(shortcuts)?
                        .with_handler(|app, shortcut, event| {
                            if event.state != ShortcutState::Pressed {
                                return;
                            }
                            if shortcut.matches(Modifiers::SUPER, Code::Comma) {
                                open_prefs(app);
                                return;
                            }
                            // overlay toggle
                            if let Some(w) = app.get_webview_window("overlay") {
                                if w.is_visible().unwrap_or(false) {
                                    let _ = w.hide();
                                } else {
                                    show_overlay(app);
                                }
                            }
                        })
                        .build(),
                )?;
            }

            // 启动后台轮询：每 3s 收集 sessions → reduce → hash 去重 → emit
            start_poll_loop(app.handle().clone());

            // 启动即显示 overlay：复用 show_overlay（位置恢复/center 兜底 + makeKey + 失焦轮询），
            // panel 居中、resident 右上角，按当前 mode 走，无需用户先点 tray。
            show_overlay(app.handle());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchored_x_keeps_right_edge() {
        // 右边 = old_x + old_w；宽度变后右边不变（50+250=300, 150+150=300, 0+300=300）
        assert_eq!(anchored_x(100.0, 200.0, 250.0), 50.0);
        assert_eq!(anchored_x(100.0, 200.0, 150.0), 150.0);
        assert_eq!(anchored_x(100.0, 200.0, 300.0), 0.0);
    }

    #[test]
    fn pos_in_rect_bounds() {
        assert!(pos_in_rect(100, 100, 0, 0, 1920, 1080));
        assert!(!pos_in_rect(2000, 100, 0, 0, 1920, 1080)); // x 超出
        assert!(!pos_in_rect(100, 2000, 0, 0, 1920, 1080)); // y 超出
        assert!(!pos_in_rect(-1, 100, 0, 0, 1920, 1080)); // x 负
                                                          // 坏坐标 186470（宽度 bug 残留）远超屏 3456 → false
        assert!(!pos_in_rect(186470, 21456, 0, 0, 3456, 2234));
    }
}
