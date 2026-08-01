// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod collector;
mod liveness;
mod models;
mod reducer;
mod statemachine;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::Emitter;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

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

/// 后台线程：每 3s collect → reduce → hash 去重 → 仅变化时 emit("sessions")。
/// Task 9 的前端监听此事件刷新 popover。
fn start_poll_loop(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut last_hash = 0u64;
        loop {
            let sessions = collector::collect_sessions();
            let merged = reducer::reduce(sessions);
            let h = hash_sessions(&merged);
            if h != last_hash {
                last_hash = h;
                let _ = handle.emit("sessions", &merged);
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .setup(|app| {
            // 构建 menubar 托盘菜单（当前仅 Quit；后续任务按需扩展）
            let quit_item =
                MenuItem::with_id(app.handle(), "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app.handle(), &[&quit_item])?;

            // tray icon 已在 tauri.conf.json 的 app.trayIcon 声明（id="main"），
            // 这里取出已存在的实例并附加菜单与点击事件。
            // Task 9 将把点击事件接入 popover 显示/隐藏；此处先占位打印。
            let tray = app.tray_by_id("main").ok_or_else(|| {
                tauri::Error::AssetNotFound("tray icon 'main'".to_string())
            })?;
            tray.set_menu(Some(menu))?;
            tray.on_tray_icon_event(|_tray, event| {
                if let tauri::tray::TrayIconEvent::Click { .. } = event {
                    println!("tray clicked");
                }
            });

            // 启动后台轮询：每 3s 收集 sessions → reduce → hash 去重 → emit
            start_poll_loop(app.handle().clone());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
