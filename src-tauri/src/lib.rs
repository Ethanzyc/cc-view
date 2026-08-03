// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod badge;
mod collector;
mod discovery;
mod focus;
mod hidden;
mod hud;
mod liveness;
mod models;
mod notify;
mod permission;
mod reducer;
mod snoozed;
mod statemachine;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
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
    }
    h.finish()
}

/// 把默认图标非透明像素染成 macOS system orange（RGB 255,159,10 = #FF9F0A），保留 alpha 通道
/// 维持抗锯齿边缘。采用方式 (b) 代码着色而非预制 PNG——无需维护第二份资源。
fn tint_orange(src: &tauri::image::Image<'_>) -> tauri::image::Image<'static> {
    let w = src.width();
    let h = src.height();
    let mut out = src.rgba().to_vec();
    // RGBA row-major，每 4 字节一像素；alpha>0 视为前景，硬置 RGB=橙。
    for px in out.chunks_exact_mut(4) {
        if px[3] > 0 {
            px[0] = 255;
            px[1] = 159;
            px[2] = 10;
            // alpha 保留
        }
    }
    tauri::image::Image::new_owned(out, w, h)
}

/// 单色 menubar 剪影（template image：黑 + 透明）。
/// include_bytes 编译期嵌入，运行时无需读盘；改图后需重新编译。
const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");

/// 后台线程：每 3s collect → reduce → notify → hash 去重 → 仅变化时 emit("sessions")。
/// 同时每轮聚合 need_attention/working 计数 → tray.set_tooltip + set_icon。
fn start_poll_loop(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut last_hash = 0u64;
        // 线程私有 notifier——observe 每轮调用以维护状态迁移
        let mut notifier = notify::Notifier::new();
        // 加载嵌入的单色剪影（template image）+ 预计算 attention 态橙色版。
        // tauri::image::Image::from_bytes 解码 PNG（需 tauri feature "image-png"）。
        let tray_icon = tauri::image::Image::from_bytes(TRAY_PNG)
            .map_err(|e| eprintln!("poll_loop: embedded tray.png decode failed: {e}"))
            .ok();
        let orange_icon = tray_icon.as_ref().map(tint_orange);
        // 跟踪 attention 状态，仅在 0↔>0 跳变时 set_icon（避免每轮 main-thread IPC 重绘）
        let mut last_attention = false;
        // perm_count 防抖：仅在等权限计数变化时重画 badge（避免每轮 main-thread IPC 重绘）
        let mut last_perm_count: usize = 0;
        loop {
            let sessions = collector::collect_sessions();
            let merged = reducer::reduce(sessions);

            // derived snoozed：每轮基于 SnoozeMap 算，随 Session emit。
            // apply_snoozed 内部锁一次 guard 共用（避免每 session 各 lock 一次）。
            let snoozed_map = handle.try_state::<Mutex<snoozed::SnoozeMap>>();
            let derived: Vec<models::Session> = apply_snoozed(&merged, snoozed_map.as_deref());

            // reduce 之后、hash 之前：每轮都 observe，让 notifier 维护状态机
            let to_notify = notifier.observe(&derived);
            for (name, status) in to_notify {
                let status_zh = match status {
                    models::Status::NeedsPermission => "等待权限确认",
                    models::Status::WaitingForInput => "等待输入",
                    _ => "需要关注",
                };
                notify::send_notification(&handle, "cc-view", &format!("{}：{}", name, status_zh));
            }

            // 聚合：need_attention = alive && !snoozed && (NeedsPermission|WaitingForInput)；
            //       working       = alive && Working
            // 排除 snoozed：snoozed = "暂时不管"，不该橙显/计入"等我"（与 notify 语义一致）。
            let need_attention = derived
                .iter()
                .filter(|s| {
                    s.alive
                        && !s.snoozed
                        && matches!(
                            s.status,
                            models::Status::NeedsPermission | models::Status::WaitingForInput
                        )
                })
                .count();
            let working = derived
                .iter()
                .filter(|s| s.alive && matches!(s.status, models::Status::Working))
                .count();

            // perm_count：等权限（硬阻塞）计数，用于 tray badge。
            // 排除 snoozed（按失效规则应已 unsnooze，保险排除）。
            let perm_count = derived
                .iter()
                .filter(|s| s.alive && !s.snoozed && matches!(s.status, models::Status::NeedsPermission))
                .count();

            if let Some(tray) = handle.tray_by_id("main") {
                // tooltip 三段：perm>0 时最前加"等权限"段（硬阻塞优先于软阻塞等我）
                let tip = if perm_count > 0 {
                    format!("{} 等权限 · {} 等我 · {} 工作", perm_count, need_attention, working)
                } else if need_attention > 0 {
                    format!("{} 等我 · {} 工作", need_attention, working)
                } else {
                    format!("{} 工作", working)
                };
                // set_tooltip 走 main-thread IPC 但开销极小，每轮更新无妨
                let _ = tray.set_tooltip(Some(tip));

                // tray icon 三态：perm>0 → badge icon（红圆数字，template=false）；
                //                  attention>0 → 橙色实色 + template=false（跳出 menubar 引起注意）；
                //                  否则单色剪影 + template=true（自动适配深浅栏）。
                // 仅在 perm_count 或 attention 跳变时 set_icon（避免每轮 main-thread IPC 重绘）。
                let has_attention = need_attention > 0;
                if perm_count != last_perm_count || has_attention != last_attention {
                    last_perm_count = perm_count;
                    last_attention = has_attention;
                    let (icon, as_template) = if perm_count > 0 {
                        // badge 合成：基于单色剪影底图画红圆数字，template=false 才能显出红色。
                        // draw_badge 返回 owned Image，无借用逃逸问题。
                        (tray_icon.as_ref().map(|img| badge::draw_badge(img, perm_count)), false)
                    } else if has_attention {
                        (orange_icon.clone(), false)
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
                    eprintln!("poll_loop: sessions cache lock poisoned");
                }
            }

            let h = hash_sessions(&derived);
            if h != last_hash {
                last_hash = h;
                let _ = handle.emit("sessions", &derived);
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

// --- Tauri commands：隐藏/取消隐藏/查询隐藏列表 ---
// lock 失败（poisoned）时 eprintln 提示 + 静默跳过——不崩溃前端调用（fail fast 可见性）。

/// 把会话加入隐藏列表并持久化。
#[tauri::command]
fn hide_session(state: tauri::State<'_, Mutex<hidden::HiddenList>>, id: String) {
    match state.lock() {
        Ok(mut h) => {
            h.add(&id);
            h.save();
        }
        Err(_) => eprintln!("hide_session: hidden state lock poisoned"),
    }
}

/// 从隐藏列表移除会话并持久化。
#[tauri::command]
fn unhide_session(state: tauri::State<'_, Mutex<hidden::HiddenList>>, id: String) {
    match state.lock() {
        Ok(mut h) => {
            h.remove(&id);
            h.save();
        }
        Err(_) => eprintln!("unhide_session: hidden state lock poisoned"),
    }
}

/// 返回当前隐藏会话 id 列表（前端据此 filter）。
#[tauri::command]
fn list_hidden(state: tauri::State<'_, Mutex<hidden::HiddenList>>) -> Vec<String> {
    state
        .lock()
        .map(|h| h.to_vec())
        .unwrap_or_else(|_| {
            eprintln!("list_hidden: hidden state lock poisoned");
            vec![]
        })
}

// --- Tauri commands：搁置/取消搁置/查询搁置表 ---
// 镜像 hide_session/unhide_session/list_hidden，但存时间戳（is_effectively_snoozed 需要）。

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
        Err(_) => eprintln!("snooze_session: snoozed state lock poisoned"),
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
        Err(_) => eprintln!("unsnooze_session: snoozed state lock poisoned"),
    }
}

/// 返回搁置表 {id: snoozedAt}（前端用于乐观更新/调试）。
#[tauri::command]
fn list_snoozed(
    state: tauri::State<'_, Mutex<snoozed::SnoozeMap>>,
) -> std::collections::HashMap<String, i64> {
    state
        .lock()
        .map(|m| m.to_map())
        .unwrap_or_else(|_| {
            eprintln!("list_snoozed: snoozed state lock poisoned");
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
        Err(_) => eprintln!("get_sessions: cache lock poisoned"),
    }
    derived
}

/// 按 session id 激活对应终端 app（MVP：只 activate，不精确定位 tab/pane）。
/// 从最近 emit 的 sessions 缓存中查 host；找不到 id 时 eprintln 提示。
#[tauri::command]
fn focus_session(id: String, cache: tauri::State<'_, Mutex<Vec<models::Session>>>) {
    match cache.lock() {
        Ok(sessions) => {
            if let Some(s) = sessions.iter().find(|s| s.id == id) {
                focus::activate_host(&s.focus_hint.host);
            } else {
                eprintln!("focus_session: session {} not in cache", id);
            }
        }
        Err(_) => eprintln!("focus_session: sessions cache lock poisoned"),
    }
}

// --- HUD always-on-top（图钉）command ---
// 后端驱动：前端不直接调 window API（免 capability 麻烦），由 command 中转 set_always_on_top。

/// 读取 HUD 是否置顶（磁盘无记录时默认 true）。
#[tauri::command]
fn get_hud_pinned() -> bool {
    hud::HudPosition::load()
        .map(|p| p.always_on_top)
        .unwrap_or(true)
}

/// 切换 HUD 置顶状态：先调原生 set_always_on_top，再把 pinned 连同现有 (x, y) 一起持久化。
#[tauri::command]
fn set_hud_pinned(pinned: bool, app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_always_on_top(pinned);
    } else {
        eprintln!("set_hud_pinned: main window not found");
    }
    let (x, y) = hud::HudPosition::load()
        .map(|p| (p.x, p.y))
        .unwrap_or((0, 0));
    hud::HudPosition::save_all(x, y, pinned);
}

/// 让 overlay 窗口加入所有 Space（含全屏 app 独占 Space）。
/// macOS 全屏应用占据独立 Space，普通 NSWindow 默认不跨 Space → 弹到桌面 Space 用户看不到。
/// Spotlight/Raycast 解法：设 NSWindowCollectionBehavior 的两个 flag：
///   - CanJoinAllSpaces    (1 << 0)   跨所有 Space 显示
///   - FullScreenAuxiliary (1 << 8)   作为全屏辅助浮层，盖在全屏 app 内容之上
/// 合计 = 1 | 256 = 257。仅对 overlay 调；HUD（main）保持默认（不跨全屏，避免干扰沉浸）。
#[cfg(target_os = "macos")]
fn join_all_spaces(w: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(ptr) = w.ns_window() else {
        eprintln!("join_all_spaces: ns_window unavailable, overlay will not cross spaces");
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
        eprintln!("overlay collectionBehavior = {} (expect 257), level = 101", val);
    }
}

/// 把 overlay 的 NSWindow isa swizzle 成 NSPanel（Spotlight/Raycast 做法）。
/// NSPanel + nonActivatingPanel 能在不激活 app 的情况下 become key 接受输入，
/// 从而不触发 Space 归属/切换——这是普通 NSWindow 跨全屏 Space 的唯一可靠解法。
/// NSPanel 是 NSWindow 子类且不加 ivar，object_setClass 安全；Tauri 的
/// show/hide/focus/vibrancy 调的都是 NSWindow 方法，swizzle 后仍正常。
#[cfg(target_os = "macos")]
fn make_panel(w: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(ptr) = w.ns_window() else {
        eprintln!("make_panel: ns_window unavailable");
        return;
    };
    let obj = ptr as *mut AnyObject;
    unsafe {
        // isa swizzle: NSWindow → NSPanel
        // ffi::object_setClass 签名: (*mut AnyObject, *const AnyClass) -> *const AnyClass
        let panel = objc2::class!(NSPanel);
        objc2::ffi::object_setClass(obj, panel as *const _);
        // NSWindowStyleMaskNonactivatingPanel = 1 << 7 (128) —— 加到现有 mask
        let mask: objc2::ffi::NSUInteger = msg_send![obj, styleMask];
        let _: () = msg_send![obj, setStyleMask: mask | (1 << 7)];
        // 不设 becomesKeyOnlyIfNeeded：它让窗口"只在需要时才 key"，导致点别处时没有
        // resign key 事件、失焦 hide 不触发（Alfred "点外面消失"需要正常 become/resign key）。
        // nonActivatingPanel 已保证 become key 不激活 app，跨全屏 Space 不受影响。
        eprintln!("overlay swizzled to NSPanel (nonActivatingPanel)");
    }
}

/// 让 overlay become key（搜索框能接受输入），但不激活 app——
/// NSPanel nonActivatingPanel 的 makeKey 不触发 NSApp activate，所以不会把同 app 的
/// HUD 窗口一起拉到最前（Tauri set_focus 会激活 app，导致 HUD 被牵连提最前）。
#[cfg(target_os = "macos")]
fn make_key(w: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    let Ok(ptr) = w.ns_window() else {
        eprintln!("make_key: ns_window unavailable");
        return;
    };
    let obj = ptr as *mut AnyObject;
    unsafe {
        // NSWindow 没有 makeKey 方法（之前误用 → selector 无效 → objc2 panic →
        // extern "C" 回调不能 unwind → abort 闪退）。正确的是 makeKeyAndOrderFront:，
        // 同时 orderFront + makeKey；对 nonActivatingPanel 不激活 app，不牵连 HUD。
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![obj, makeKeyAndOrderFront: nil];
    }
}

/// 返回当前前台 app 的 bundle id（NSWorkspace.sharedWorkspace.frontmostApplication.bundleIdentifier）。
/// 用于 overlay 失焦检测：NSPanel nonActivatingPanel 不触发 Focused(false)，
/// 改查 frontmost app 是否变化（变了 = 用户切到别的 app）。cc-view 自身是 accessory app
/// （LSUIElement）从不是 frontmost，所以基准值是用户呼出时所在的 app。
#[cfg(target_os = "macos")]
fn frontmost_bundle_id() -> Option<String> {
    use objc2::{class, msg_send};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(Mutex::new(hidden::HiddenList::load()))
        .manage(Mutex::new(snoozed::SnoozeMap::load()))
        .manage(Mutex::new(Vec::<models::Session>::new()))
        .invoke_handler(tauri::generate_handler![
            hide_session,
            unhide_session,
            list_hidden,
            focus_session,
            get_sessions,
            get_hud_pinned,
            set_hud_pinned,
            snooze_session,
            unsnooze_session,
            list_snoozed
        ])
        .setup(|app| {
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

            // 给 popover 窗口设原生 vibrancy（NSVisualEffectView，系统渲染）。
            // 替代 CSS backdrop-filter：桌面变化时背景稳定，且自适应明暗主题。
            // HudWindow material：HUD 面板专用，比 Popover 更暗更不透明——深色模式下
            // Popover 偏中灰透桌面亮色，灰阶文字（muted/tertiary）对比不足发糊。radius 8 与 .app 对齐。
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_effects(
                    EffectsBuilder::new()
                        .effect(Effect::HudWindow)
                        .state(EffectState::Active)
                        .radius(8.)
                        .build(),
                );

                // 恢复上次保存的 HUD 位置（vibrancy 之后）。
                // 找不到 / 失败时静默使用 tauri.conf.json 默认位置。
                if let Some(pos) = hud::HudPosition::load() {
                    let _ = w.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
                    // 按记忆的 always_on_top 设置（默认 true，保持现有行为）
                    let _ = w.set_always_on_top(pos.always_on_top);
                }

                // 拖动 HUD 后存位置：WindowEvent::Moved 携带 PhysicalPosition<i32>，
                // 直接传给 hud::HudPosition::save 即可（无需 cast）。
                // on_window_event 接收 Fn(&WindowEvent) + Send + 'static。
                w.on_window_event(|e| {
                    if let tauri::WindowEvent::Moved(p) = e {
                        hud::HudPosition::save(p.x, p.y);
                    }
                });
            }

            // overlay 窗口：失焦自动 hide（Alfred/uTools 行为——点别处就收起）。
            // on_window_event 闭包签名是 Fn(&WindowEvent)（单参），拿不到 window 引用——
            // 外层 clone WebviewWindow（Tauri 2 派生 Clone，是廉价 handle 非拥有资源）
            // 再 move 进闭包，失焦时调 hide()。仅 overlay 有此行为；HUD（main）常驻不 hide。
            // 同时套同款 Popover vibrancy——与 HUD 视觉一致；radius 12 比 main 略大，
            // 命令面板观感更柔和。EffectState::Active 保证失焦时仍保持毛玻璃（不灰化）。
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.set_effects(
                    EffectsBuilder::new()
                        .effect(Effect::Popover)
                        .state(EffectState::Active)
                        .radius(12.)
                        .build(),
                );

                // 跨 Space / 全屏可见：overlay 需在全屏 app 下也能弹出（Spotlight/Raycast 行为）
                #[cfg(target_os = "macos")]
                join_all_spaces(&overlay);

                // isa swizzle NSWindow → NSPanel（一次性）：
                // nonActivatingPanel 才能真正跨全屏 Space（become key 不激活 app → 不切 Space）。
                // 必须在 vibriosity set_effects 之后、窗口 show 之前调。
                #[cfg(target_os = "macos")]
                make_panel(&overlay);

                let w = overlay.clone();
                overlay.on_window_event(move |e| {
                    if let tauri::WindowEvent::Focused(false) = e {
                        let _ = w.hide();
                    }
                });
            }

            // 构建 menubar 托盘菜单（当前仅 Quit；后续任务按需扩展）
            let quit_item =
                MenuItem::with_id(app.handle(), "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app.handle(), &[&quit_item])?;

            // tray icon 已在 tauri.conf.json 的 app.trayIcon 声明（id="main"），
            // 这里取出已存在的实例并附加菜单与点击事件。
            // 左键点击 toggle HUD 窗口显示/隐藏——位置由用户拖动记忆，不再贴 tray。
            // 同时去掉 set_focus 避免抢走当前焦点（终端输入）。
            let tray = app.tray_by_id("main").ok_or_else(|| {
                tauri::Error::AssetNotFound("tray icon 'main'".to_string())
            })?;
            tray.set_menu(Some(menu))?;
            tray.on_tray_icon_event(|tray, event| {
                if let tauri::tray::TrayIconEvent::Click {
                    button: tauri::tray::MouseButton::Left,
                    button_state: tauri::tray::MouseButtonState::Up,
                    ..
                } = event
                {
                    let app = tray.app_handle();
                    if let Some(w) = app.get_webview_window("main") {
                        // 点图标总是把 HUD 调到最前（show + set_focus）。
                        // 之前是 toggle（可见→hide），但 HUD 被别的 app 挡住时仍是 visible，
                        // 点图标会走 hide 分支收起——不符合"被挡住时点图标拉到最前"的直觉。
                        // show 对已 visible 的窗口相当于 orderFront（提最前），对 hide 的则显示。
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            });

            // 注册 ⌥Space 全局快捷键 → toggle overlay 窗口。
            // 2.x API（v2.3.2 实测）：没有 init() 工厂函数，须用 Builder 模式在 setup 内
            // 通过 app.handle().plugin(...) 动态注册。brief 提到的 register(s).on_shortcut(...)
            // 链式调用也无效（register 返回 Result<()>）。正确路径是 with_shortcuts +
            // with_handler 一步到位（参考 v2 README）。
            // Builder / shortcut 注册失败时 ? 向上传播（setup 返回 Box<dyn Error>）——fail fast。
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{
                    Builder, Code, Modifiers, ShortcutState,
                };
                app.handle().plugin(
                    Builder::new()
                        .with_shortcuts(["alt+space"])?
                        .with_handler(|app, shortcut, event| {
                            if event.state == ShortcutState::Pressed
                                && shortcut.matches(Modifiers::ALT, Code::Space)
                            {
                                if let Some(w) = app.get_webview_window("overlay") {
                                    if w.is_visible().unwrap_or(false) {
                                        let _ = w.hide();
                                    } else {
                                        // 呼出时抢焦点（overlay 用于搜索输入，区别于 HUD 不抢焦点）
                                        // 关键：show() 之前先设 collectionBehavior + level——
                                        // 否则 show 那刻窗口被 macOS 钉在桌面 Space（事后设也来不及）。
                                        #[cfg(target_os = "macos")]
                                        join_all_spaces(&w);
                                        // 每次 show 前居中——即使上次拖动过，呼出总在屏幕中心
                                        let _ = w.center();
                                        let _ = w.show();
                                        // 用原生 makeKey 而非 set_focus：后者激活 cc-view app，
                                        // 会把 HUD 一起拉到最前；NSPanel makeKey 不激活 app。
                                        #[cfg(target_os = "macos")]
                                        make_key(&w);
                                        #[cfg(not(target_os = "macos"))]
                                        let _ = w.set_focus();
                                        // show/set_focus 后再设一次，防 Tauri 内部重置。
                                        #[cfg(target_os = "macos")]
                                        join_all_spaces(&w);

                                        // show 后启动失焦检测轮询（Alfred/Raycast 做法）：
                                        // NSPanel nonActivatingPanel 点别的 app 时不 resign key →
                                        // on_window_event(Focused(false)) 不触发。改每 200ms 查 frontmost app，
                                        // 一旦变了（用户切到别的 app）→ hide overlay。
                                        // overlay 不再 visible 时（别的方式收起）轮询自动退出，无泄漏。
                                        #[cfg(target_os = "macos")]
                                        {
                                            let app_handle = app.clone();
                                            std::thread::spawn(move || {
                                                // 先等 show/set_focus 稳定——首次激活 cc-view 会短暂改变
                                                // frontmost，立即比较会误判"切走"导致 overlay 闪一下消失。
                                                std::thread::sleep(
                                                    std::time::Duration::from_millis(300),
                                                );
                                                // 稳定后的基准前台 app（之后变了 = 用户切走）
                                                let stable_front = frontmost_bundle_id();
                                                loop {
                                                    std::thread::sleep(
                                                        std::time::Duration::from_millis(200),
                                                    );
                                                    let Some(w) =
                                                        app_handle.get_webview_window("overlay")
                                                    else {
                                                        break;
                                                    };
                                                    // overlay 已 hide（别的方式收起）→ 退出
                                                    if !w.is_visible().unwrap_or(false) {
                                                        break;
                                                    }
                                                    // 前台 app 变了（用户切到别的 app）→ hide
                                                    if frontmost_bundle_id() != stable_front {
                                                        let _ = w.hide();
                                                        break;
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        })
                        .build(),
                )?;
            }

            // 启动后台轮询：每 3s 收集 sessions → reduce → hash 去重 → emit
            start_poll_loop(app.handle().clone());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
