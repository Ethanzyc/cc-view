// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod collector;
mod discovery;
mod focus;
mod hidden;
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

/// 后台线程：每 3s collect → reduce → notify → hash 去重 → 仅变化时 emit("sessions")。
/// Task 9 的前端监听此事件刷新 popover。
fn start_poll_loop(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut last_hash = 0u64;
        // 线程私有 notifier——observe 每轮调用以维护状态迁移
        let mut notifier = notify::Notifier::new();
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
            let h = hash_sessions(&merged);
            if h != last_hash {
                last_hash = h;
                // emit 前刷新 sessions 缓存，供 focus_session command 查询 host。
                // 用 app handle 取 state（与 hidden 的 Mutex 并列管理）。
                if let Some(cache) = handle.try_state::<Mutex<Vec<models::Session>>>() {
                    if let Ok(mut c) = cache.lock() {
                        *c = merged.clone();
                    } else {
                        eprintln!("poll_loop: sessions cache lock poisoned");
                    }
                }
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
            focus_session
        ])
        .setup(|app| {
            // 构建 menubar 托盘菜单（当前仅 Quit；后续任务按需扩展）
            let quit_item =
                MenuItem::with_id(app.handle(), "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app.handle(), &[&quit_item])?;

            // tray icon 已在 tauri.conf.json 的 app.trayIcon 声明（id="main"），
            // 这里取出已存在的实例并附加菜单与点击事件。
            // 左键点击 toggle popover 窗口（label "main"，与 capabilities 对齐）。
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
                            let _ = w.set_focus();
                        }
                    }
                }
            });

            // 启动后台轮询：每 3s 收集 sessions → reduce → hash 去重 → emit
            start_poll_loop(app.handle().clone());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
