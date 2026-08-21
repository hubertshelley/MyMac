use std::sync::{Arc, Mutex};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

mod apps;
mod config;
mod launch;
mod status;
mod system;

use system::{AppState, SystemMonitor};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let monitor = Arc::new(Mutex::new(SystemMonitor::new()));
            let config = Arc::new(Mutex::new(config::load_config()));

            // 初始状态栏图标
            let init_icon = {
                let m = monitor.lock().unwrap();
                let cfg = config.lock().unwrap();
                status::render_status_icon(&m.snapshot(), &cfg)
            };

            // 状态栏菜单项（前三项为实时信息，禁用点击）
            let cpu_item = MenuItem::with_id(app, "cpu", "CPU：--", false, None::<&str>)?;
            let mem_item = MenuItem::with_id(app, "mem", "内存：--", false, None::<&str>)?;
            let net_item = MenuItem::with_id(app, "net", "网络：--", false, None::<&str>)?;
            let show_item = MenuItem::with_id(app, "show", "打开主面板", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 MyMac", true, None::<&str>)?;

            let menu = Menu::with_items(
                app,
                &[
                    &cpu_item,
                    &mem_item,
                    &net_item,
                    &PredefinedMenuItem::separator(app)?,
                    &show_item,
                    &quit_item,
                ],
            )?;

            let tray = TrayIconBuilder::with_id("main-tray")
                .icon(init_icon)
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

            // 后台线程：每 2 秒采集信息，更新状态栏图标与菜单文案
            let monitor_clone = monitor.clone();
            let config_clone = config.clone();
            let tray_clone = tray.clone();
            let cpu_item = cpu_item.clone();
            let mem_item = mem_item.clone();
            let net_item = net_item.clone();
            std::thread::spawn(move || loop {
                let snapshot = {
                    let mut m = monitor_clone.lock().unwrap();
                    m.refresh();
                    m.snapshot()
                };
                let cfg = config_clone.lock().unwrap().clone();

                let _ = cpu_item.set_text(format!("CPU：{:.1}%", snapshot.cpu_usage));
                let _ = mem_item.set_text(format!(
                    "内存：{:.1}%（{} / {}）",
                    snapshot.memory_usage,
                    format_bytes(snapshot.memory_used),
                    format_bytes(snapshot.memory_total)
                ));

                let net_text = format!(
                    "网络：↓ {} · ↑ {}",
                    status::format_rate(snapshot.net_down_rate),
                    status::format_rate(snapshot.net_up_rate)
                );
                let _ = net_item.set_text(net_text);

                let icon = status::render_status_icon(&snapshot, &cfg);
                let _ = tray_clone.set_icon(Some(icon));

                std::thread::sleep(std::time::Duration::from_secs(2));
            });

            app.manage(AppState { monitor, config });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system::get_system_info,
            apps::list_apps,
            apps::uninstall_app,
            launch::list_launch_items,
            launch::set_launch_item,
            launch::delete_launch_item,
            config::get_status_config,
            config::set_status_config,
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

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let i = ((bytes as f64).log2() / 10.0).floor() as usize;
    let i = i.min(UNITS.len() - 1);
    let v = bytes as f64 / 1024f64.powi(i as i32);
    format!("{v:.1} {}", UNITS[i])
}
