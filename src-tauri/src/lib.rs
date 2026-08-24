use std::sync::{Arc, Mutex};
use tauri::{
    menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

mod apps;
mod clipboard;
mod config;
mod launch;
mod status;
mod system;

use clipboard::ClipboardState;
use system::{AppState, SystemMonitor};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let monitor = Arc::new(Mutex::new(SystemMonitor::new()));
            let config = Arc::new(Mutex::new(config::load_config()));

            // 剪贴板历史状态（启动时加载本地记录，并记录当前剪贴板指纹，
            // 避免重启后残留内容被当作新记录重新写入）
            let clip_state = ClipboardState {
                items: Mutex::new(clipboard::load_history()),
                last_seen: Mutex::new(clipboard::current_clipboard_fingerprint()),
            };

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

            // 粘贴板历史子菜单（动态刷新最近记录）
            let clip_submenu = Submenu::with_id(app, "clip-history", "粘贴板历史", true)?;
            rebuild_clip_menu(app.handle(), &clip_submenu, &clip_state);

            let menu = Menu::with_items(
                app,
                &[
                    &cpu_item,
                    &mem_item,
                    &net_item,
                    &PredefinedMenuItem::separator(app)?,
                    &clip_submenu,
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
                .on_menu_event({
                    let clip_submenu = clip_submenu.clone();
                    move |app, event| match event.id.as_ref() {
                        "show" => show_main_window(app),
                        "quit" => app.exit(0),
                        "clear-clip" => {
                            let state = app.state::<ClipboardState>();
                            let _ = clipboard::clear_history(state.inner());
                            rebuild_clip_menu(app, &clip_submenu, state.inner());
                            let _ = app.emit("clip-history-changed", ());
                        }
                        id if id.starts_with("clip-item-") => {
                            let clip_id = id.trim_start_matches("clip-item-");
                            let state = app.state::<ClipboardState>();
                            let _ = clipboard::copy_to_clipboard_and_top(state.inner(), clip_id);
                            rebuild_clip_menu(app, &clip_submenu, state.inner());
                            let _ = app.emit("clip-history-changed", ());
                        }
                        _ => {}
                    }
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

            // 关闭主窗口时隐藏窗口与 Dock 图标，状态栏继续常驻
            if let Some(window) = app.get_webview_window("main") {
                let window_for_close = window.clone();
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_for_close.hide();
                        #[cfg(target_os = "macos")]
                        let _ = app_handle.set_dock_visibility(false);
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

            // 后台线程：监听剪贴板变化，写入历史并刷新状态栏子菜单
            let watcher_app = app.handle().clone();
            let watcher_submenu = clip_submenu.clone();
            std::thread::spawn(move || {
                let mut cb = match arboard::Clipboard::new() {
                    Ok(cb) => cb,
                    Err(_) => return,
                };
                loop {
                    // 优先读取文本，无文本时读取图片
                    let content = if let Ok(text) = cb.get_text() {
                        if text.trim().is_empty() {
                            None
                        } else {
                            Some(clipboard::NewContent::Text(text))
                        }
                    } else if let Ok(img) = cb.get_image() {
                        if img.bytes.is_empty() {
                            None
                        } else {
                            Some(clipboard::NewContent::Image {
                                width: img.width as u32,
                                height: img.height as u32,
                                rgba: img.bytes.into_owned(),
                            })
                        }
                    } else {
                        None
                    };

                    if let Some(content) = content {
                        let state = watcher_app.state::<ClipboardState>();
                        let changed = {
                            let mut items = state.items.lock().unwrap();
                            let mut last_seen = state.last_seen.lock().unwrap();
                            let changed =
                                clipboard::upsert(&mut items, content, &mut last_seen);
                            if changed {
                                let snapshot = items.clone();
                                drop(items);
                                let _ = clipboard::save_history(&snapshot);
                            }
                            changed
                        };
                        if changed {
                            rebuild_clip_menu(&watcher_app, &watcher_submenu, state.inner());
                            let _ = watcher_app.emit("clip-history-changed", ());
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            });

            app.manage(AppState { monitor, config });
            app.manage(clip_state);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system::get_system_info,
            apps::list_apps,
            apps::scan_app_related,
            apps::uninstall_app_items,
            launch::list_launch_items,
            launch::set_launch_item,
            launch::delete_launch_item,
            launch::reveal_launch_item,
            config::get_status_config,
            config::set_status_config,
            clipboard::get_clip_history,
            clipboard::get_clip_image,
            clipboard::delete_clip_item,
            clipboard::clear_clip_history,
            clipboard::copy_clip_item,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 重建状态栏「粘贴板历史」子菜单内容
fn rebuild_clip_menu(
    app: &tauri::AppHandle,
    submenu: &Submenu<tauri::Wry>,
    state: &ClipboardState,
) {
    let items = state.items.lock().unwrap();

    // 移除子菜单中现有项，再整体重建
    if let Ok(existing) = submenu.items() {
        for item in existing {
            let _ = submenu.remove(&item);
        }
    }

    let mut owned: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = Vec::new();

    if items.is_empty() {
        if let Ok(mi) =
            MenuItem::with_id(app, "clip-empty", "暂无记录", false, None::<&str>)
        {
            owned.push(Box::new(mi));
        }
    } else {
        for item in items.iter().take(clipboard::TRAY_SHOW_ITEMS) {
            if let Ok(mi) = MenuItem::with_id(
                app,
                format!("clip-item-{}", item.id),
                clipboard::menu_label(item),
                true,
                None::<&str>,
            ) {
                owned.push(Box::new(mi));
            }
        }
        if let Ok(sep) = PredefinedMenuItem::separator(app) {
            owned.push(Box::new(sep));
        }
        if let Ok(clear) =
            MenuItem::with_id(app, "clear-clip", "清空粘贴板历史", true, None::<&str>)
        {
            owned.push(Box::new(clear));
        }
    }

    let refs: Vec<&dyn IsMenuItem<tauri::Wry>> =
        owned.iter().map(|b| b.as_ref()).collect();
    let _ = submenu.append_items(&refs);
}

fn show_main_window(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_dock_visibility(true);

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