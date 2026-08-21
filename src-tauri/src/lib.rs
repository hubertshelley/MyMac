use std::sync::{Arc, Mutex};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

mod apps;
mod launch;
mod system;

use system::{AppState, SystemMonitor};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let monitor = Arc::new(Mutex::new(SystemMonitor::new()));

            // 状态栏菜单项
            let cpu_item = MenuItem::with_id(app, "cpu", "CPU：--", true, None::<&str>)?;
            let mem_item = MenuItem::with_id(app, "mem", "内存：--", true, None::<&str>)?;
            let show_item = MenuItem::with_id(app, "show", "打开主面板", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 MyMac", true, None::<&str>)?;

            let menu = Menu::with_items(
                app,
                &[
                    &cpu_item,
                    &mem_item,
                    &PredefinedMenuItem::separator(app)?,
                    &show_item,
                    &quit_item,
                ],
            )?;

            let tray = TrayIconBuilder::with_id("main-tray")
                .tooltip("MyMac 电脑管家")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            if let Some(icon) = app.default_window_icon() {
                let _ = tray.set_icon(Some(icon.clone()));
            }

            // 关闭窗口时隐藏而非退出，保持常驻菜单栏
            if let Some(window) = app.get_webview_window("main") {
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            // 后台线程：每秒采集一次系统信息并更新菜单栏文案
            let monitor_clone = monitor.clone();
            let cpu_item = cpu_item.clone();
            let mem_item = mem_item.clone();
            std::thread::spawn(move || loop {
                let (cpu, mem) = {
                    let mut m = monitor_clone.lock().unwrap();
                    m.refresh();
                    let info = m.snapshot();
                    (info.cpu_usage, info.memory_usage)
                };
                let _ = cpu_item.set_text(format!("CPU：{cpu:.1}%"));
                let _ = mem_item.set_text(format!("内存：{mem:.1}%"));
                std::thread::sleep(std::time::Duration::from_secs(1));
            });

            app.manage(AppState { monitor });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system::get_system_info,
            apps::list_apps,
            apps::uninstall_app,
            launch::list_launch_items,
            launch::set_launch_item,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
