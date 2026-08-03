// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
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
mod statemachine;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{Emitter, Manager};

/// 对会话列表做内容 hash，仅按 (id, status, alive) 维度。
/// 只要这三项不变就不 emit，避免每 3s 给前端刷一屏。
fn hash_sessions(s: &[models::Session]) -> u64 {
    let mut h = DefaultHasher::new();
    for x in s {
        x.id.hash(&mut h);
        format!("{:?}", x.status).hash(&mut h);
        x.alive.hash(&mut h);
    }
    h.finish()
}

/// 把默认图标非透明像素染成 macOS system orange（RGB 255,149,0），保留 alpha 通道
/// 维持抗锯齿边缘。采用方式 (b) 代码着色而非预制 PNG——无需维护第二份资源。
fn tint_orange(src: &tauri::image::Image<'_>) -> tauri::image::Image<'static> {
    let w = src.width();
    let h = src.height();
    let mut out = src.rgba().to_vec();
    // RGBA row-major，每 4 字节一像素；alpha>0 视为前景，硬置 RGB=橙。
    for px in out.chunks_exact_mut(4) {
        if px[3] > 0 {
            px[0] = 255;
            px[1] = 149;
            px[2] = 0;
            // alpha 保留
        }
    }
    tauri::image::Image::new_owned(out, w, h)
}

/// 后台线程：每 3s collect → reduce → notify → hash 去重 → 仅变化时 emit("sessions")。
/// 同时每轮聚合 need_attention/working 计数 → tray.set_tooltip + set_icon。
fn start_poll_loop(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut last_hash = 0u64;
        // 线程私有 notifier——observe 每轮调用以维护状态迁移
        let mut notifier = notify::Notifier::new();
        // 预计算两份图标：默认（活→无 attention 时）+ 橙（有 attention 时）。
        // default_window_icon 返回 &Image<'a>（借 handle），先 cloned 再 to_owned 拿到
        // Image<'static>（rgba owned）——后续循环里不再借 handle。参考 tauri app/plugin.rs。
        let default_icon: Option<tauri::image::Image<'static>> = handle
            .default_window_icon()
            .cloned()
            .map(tauri::image::Image::to_owned);
        let orange_icon = default_icon.as_ref().map(tint_orange);
        if default_icon.is_none() {
            eprintln!("poll_loop: default_window_icon missing, set_icon disabled");
        }
        // 跟踪 attention 状态，仅在 0↔>0 跳变时 set_icon（避免每轮 main-thread IPC 重绘）
        let mut last_attention = false;
        loop {
            let sessions = collector::collect_sessions();
            let merged = reducer::reduce(sessions);
            // reduce 之后、hash 之前：每轮都 observe，让 notifier 维护状态机
            let to_notify = notifier.observe(&merged);
            for (name, status) in to_notify {
                let status_zh = match status {
                    models::Status::NeedsPermission => "等待权限确认",
                    models::Status::WaitingForInput => "等待输入",
                    _ => "需要关注",
                };
                notify::send_notification("cc-view", &format!("{}：{}", name, status_zh));
            }

            // 聚合：need_attention = alive && (NeedsPermission|WaitingForInput)；
            //       working       = alive && Working
            let need_attention = merged
                .iter()
                .filter(|s| {
                    s.alive
                        && matches!(
                            s.status,
                            models::Status::NeedsPermission | models::Status::WaitingForInput
                        )
                })
                .count();
            let working = merged
                .iter()
                .filter(|s| s.alive && matches!(s.status, models::Status::Working))
                .count();

            // tooltip：need>0 时双段"N 等我 · M 工作"，否则只显示"M 工作"
            let tip = if need_attention > 0 {
                format!("{} 等我 · {} 工作", need_attention, working)
            } else {
                format!("{} 工作", working)
            };

            if let Some(tray) = handle.tray_by_id("main") {
                // set_tooltip 走 main-thread IPC 但开销极小，每轮更新无妨
                let _ = tray.set_tooltip(Some(tip));
                // set_icon 仅在 attention 状态跳变时调用（0↔>0），避免无谓重绘
                let has_attention = need_attention > 0;
                if has_attention != last_attention {
                    last_attention = has_attention;
                    let img = if has_attention {
                        orange_icon.clone()
                    } else {
                        default_icon.clone()
                    };
                    if let Some(img) = img {
                        let _ = tray.set_icon(Some(img));
                    }
                }
            }

            // 每轮刷新 sessions 缓存，供 focus_session command 查询 host。
            // cache 每轮更新，但 emit 仍受 hash 去重控制——稳定期也拿到最新 host。
            if let Some(cache) = handle.try_state::<Mutex<Vec<models::Session>>>() {
                if let Ok(mut c) = cache.lock() {
                    *c = merged.clone();
                } else {
                    eprintln!("poll_loop: sessions cache lock poisoned");
                }
            }

            let h = hash_sessions(&merged);
            if h != last_hash {
                last_hash = h;
                let _ = handle.emit("sessions", &merged);
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

/// 立即采集并返回当前所有会话（供前端打开时拉初始数据 / 手动刷新）。
/// 不依赖 poll loop 的 3s 节拍与 hash 去重——调用即拿实时结果。
/// 同时刷新 cache，让 focus_session 也能用到最新 host。
#[tauri::command]
fn get_sessions(cache: tauri::State<'_, Mutex<Vec<models::Session>>>) -> Vec<models::Session> {
    let merged = reducer::reduce(collector::collect_sessions());
    match cache.lock() {
        Ok(mut c) => *c = merged.clone(),
        Err(_) => eprintln!("get_sessions: cache lock poisoned"),
    }
    merged
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
    unsafe {
        let _: () = msg_send![obj, setCollectionBehavior: behavior];
        // 读回 collectionBehavior 确认是否真设上（诊断用：重启后看终端日志）。
        // 怀疑 Tauri show()/set_focus() 会重置 collectionBehavior——此日志用于比对。
        let val: objc2::ffi::NSUInteger = msg_send![obj, collectionBehavior];
        eprintln!("overlay collectionBehavior = {} (expect 257)", val);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(hidden::HiddenList::load()))
        .manage(Mutex::new(Vec::<models::Session>::new()))
        .invoke_handler(tauri::generate_handler![
            hide_session,
            unhide_session,
            list_hidden,
            focus_session,
            get_sessions,
            get_hud_pinned,
            set_hud_pinned
        ])
        .setup(|app| {
            // 给 popover 窗口设原生 vibrancy（NSVisualEffectView，系统渲染）。
            // 替代 CSS backdrop-filter：桌面变化时背景稳定，且自适应明暗主题。
            // Popover material 语义匹配 menubar popover；radius 8 与 .app CSS border-radius 对齐。
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_effects(
                    EffectsBuilder::new()
                        .effect(Effect::Popover)
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
                        if w.is_visible().unwrap_or(false) {
                            let _ = w.hide();
                        } else {
                            let _ = w.show();
                        }
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
                                        // 每次 show 前居中——即使上次拖动过，呼出总在屏幕中心
                                        let _ = w.center();
                                        let _ = w.show();
                                        let _ = w.set_focus();
                                        // Tauri show()/set_focus() 可能重置 collectionBehavior，
                                        // 每次弹出都重设，保证全屏 app 下 overlay 跨 Space 可见。
                                        #[cfg(target_os = "macos")]
                                        join_all_spaces(&w);
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
