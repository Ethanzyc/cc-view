// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod collector;
mod liveness;
mod models;
mod statemachine;

use tauri::menu::{Menu, MenuItem};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
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

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
