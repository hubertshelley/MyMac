use std::sync::{Arc, Mutex};
use tauri::{
    menu::{IconMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

mod apps;
mod brew;
mod clipboard;
mod config;
mod launch;
mod status;
mod system;
mod totp;

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
                // 启动时不读取剪贴板内容，避免应用启动触发通用剪贴板按需取回。
                last_seen: Mutex::new(None),
            };
            let totp_state = totp::TotpState {
                accounts: Mutex::new(totp::load_accounts()),
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

            // 2FA 验证码子菜单（验证码随刷新线程更新）
            let totp_submenu = Submenu::with_id(app, "totp-codes", "2FA 验证码", true)?;
            rebuild_totp_menu(app.handle(), &totp_submenu, &totp_state);

            let menu = Menu::with_items(
                app,
                &[
                    &cpu_item,
                    &mem_item,
                    &net_item,
                    &PredefinedMenuItem::separator(app)?,
                    &clip_submenu,
                    &totp_submenu,
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
                        "manage-totp" => {
                            show_main_window(app);
                            let _ = app.emit("navigate-to", "totp");
                        }
                        "quit" => app.exit(0),
                        id if id.starts_with("totp-item-") => {
                            let account_id = id.trim_start_matches("totp-item-").to_string();
                            let app = app.clone();
                            std::thread::spawn(move || {
                                let totp_state = app.state::<totp::TotpState>();
                                let clip_state = app.state::<ClipboardState>();
                                match totp::copy_code(
                                    totp_state.inner(),
                                    clip_state.inner(),
                                    &account_id,
                                ) {
                                    Ok(_) => {
                                        let _ = app.emit("totp-code-copied", account_id);
                                    }
                                    Err(error) => {
                                        eprintln!("状态栏复制 2FA 验证码失败：{error}");
                                        let _ = app.emit("totp-copy-failed", error);
                                    }
                                }
                            });
                        }
                        // 剪贴板读写与菜单重建在后台线程执行，避免阻塞主线程
                        "clear-clip" => {
                            let app = app.clone();
                            let submenu = clip_submenu.clone();
                            std::thread::spawn(move || {
                                let state = app.state::<ClipboardState>();
                                let _ = clipboard::clear_history(state.inner());
                                rebuild_clip_menu(&app, &submenu, state.inner());
                                let _ = app.emit("clip-history-changed", ());
                            });
                        }
                        id if id.starts_with("clip-item-") => {
                            let clip_id = id.trim_start_matches("clip-item-").to_string();
                            let app = app.clone();
                            let submenu = clip_submenu.clone();
                            std::thread::spawn(move || {
                                let state = app.state::<ClipboardState>();
                                let _ =
                                    clipboard::copy_to_clipboard_and_top(state.inner(), &clip_id);
                                rebuild_clip_menu(&app, &submenu, state.inner());
                                let _ = app.emit("clip-history-changed", ());
                            });
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

            // 注册共享状态后再启动会读取状态的后台线程
            app.manage(AppState {
                monitor: monitor.clone(),
                config: config.clone(),
            });
            app.manage(clip_state);
            app.manage(totp_state);

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
            let status_app = app.handle().clone();
            let status_totp_submenu = totp_submenu.clone();
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
                let totp_state = status_app.state::<totp::TotpState>();
                update_totp_menu(&status_app, &status_totp_submenu, totp_state.inner());

                std::thread::sleep(std::time::Duration::from_secs(2));
            });

            // 后台线程：监听剪贴板变化，写入历史并刷新状态栏子菜单
            let watcher_app = app.handle().clone();
            let watcher_submenu = clip_submenu.clone();
            std::thread::spawn(move || {
                // 常驻线程不持有剪贴板读取对象，只观察轻量的变化编号。
                let mut last_change_count = clipboard::clipboard_change_count();
                loop {
                    let current_change_count = clipboard::clipboard_change_count();
                    if current_change_count.is_some() && current_change_count == last_change_count {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        continue;
                    }
                    last_change_count = current_change_count;

                    // 通用剪贴板可能先发布占位项，再异步取回内容；等待变化稳定后再读取。
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(350));
                        let observed = clipboard::clipboard_change_count();
                        if observed == last_change_count {
                            break;
                        }
                        last_change_count = observed;
                    }
                    // 远程内容先留给用户实际粘贴；之后若剪贴板仍未变化，再单次读取并纳入历史。
                    if clipboard::is_remote_clipboard() {
                        let remote_change_count = last_change_count;
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        if clipboard::clipboard_change_count() != remote_change_count {
                            continue;
                        }
                    }
                    let mut cb = match arboard::Clipboard::new() {
                        Ok(cb) => cb,
                        Err(_) => continue,
                    };

                    // 仅在剪贴板确实变化并稳定后读取一次；优先文本，无文本时读取图片。
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
                            // 快速检查：内容与上次一致则跳过，避免重复保存图片文件
                            let fp =
                                clipboard::image_fingerprint(img.width, img.height, &img.bytes);
                            let already_seen = {
                                let state = watcher_app.state::<ClipboardState>();
                                let last_seen = state.last_seen.lock().unwrap();
                                last_seen.as_deref() == Some(fp.as_str())
                            };
                            if already_seen {
                                None
                            } else {
                                // 锁外保存图片文件（解码、缩略图生成耗时，避免持锁）
                                clipboard::prepare_image(
                                    img.width as u32,
                                    img.height as u32,
                                    &img.bytes,
                                )
                                .map(clipboard::NewContent::Image)
                            }
                        }
                    } else {
                        None
                    };

                    if let Some(content) = content {
                        let state = watcher_app.state::<ClipboardState>();
                        let changed = {
                            let mut items = state.items.lock().unwrap();
                            let mut last_seen = state.last_seen.lock().unwrap();
                            let changed = clipboard::upsert(&mut items, content, &mut last_seen);
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
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system::get_system_info,
            apps::list_apps,
            apps::scan_app_related,
            apps::uninstall_app_items,
            brew::get_brew_status,
            brew::start_brew_install,
            brew::set_brew_source,
            brew::list_brew_packages,
            brew::search_brew_packages,
            brew::install_brew_package,
            brew::uninstall_brew_package,
            brew::upgrade_brew_package,
            brew::upgrade_all_brew_packages,
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
            totp::get_totp_accounts,
            totp::add_totp_account,
            totp::delete_totp_account,
            totp::copy_totp_code,
            totp::decode_totp_qr_image,
            totp::capture_totp_qr,
            totp::decode_totp_qr_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 重建状态栏「粘贴板历史」子菜单内容
/// 注意：子菜单增删操作会同步派发到主线程执行，因此必须先克隆数据、
/// 释放 items 锁后再操作，避免与主线程的菜单事件处理形成死锁。
fn rebuild_clip_menu(
    app: &tauri::AppHandle,
    submenu: &Submenu<tauri::Wry>,
    state: &ClipboardState,
) {
    // 1. 克隆菜单数据（短暂持锁，克隆后立即释放）
    let menu_data: Vec<(String, String, Option<String>)> = {
        let items = state.items.lock().unwrap();
        items
            .iter()
            .take(clipboard::TRAY_SHOW_ITEMS)
            .map(|i| (i.id.clone(), clipboard::menu_label(i), i.thumb_file.clone()))
            .collect()
    };
    let empty = menu_data.is_empty();

    // 2. 释放锁后操作子菜单（以下调用会 dispatch 到主线程）
    if let Ok(existing) = submenu.items() {
        for item in existing {
            let _ = submenu.remove(&item);
        }
    }

    let mut owned: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = Vec::new();
    if empty {
        if let Ok(mi) = MenuItem::with_id(app, "clip-empty", "暂无记录", false, None::<&str>) {
            owned.push(Box::new(mi));
        }
    } else {
        for (id, label, thumb) in menu_data {
            if let Some(thumb) = thumb {
                // 图片记录：带缩略图图标
                let icon = clipboard::load_thumb_image(&thumb);
                if let Ok(mi) = IconMenuItem::with_id(
                    app,
                    format!("clip-item-{id}"),
                    label,
                    true,
                    icon,
                    None::<&str>,
                ) {
                    owned.push(Box::new(mi));
                }
            } else if let Ok(mi) =
                MenuItem::with_id(app, format!("clip-item-{id}"), label, true, None::<&str>)
            {
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

    let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = owned.iter().map(|b| b.as_ref()).collect();
    let _ = submenu.append_items(&refs);
}

fn update_totp_menu(
    app: &tauri::AppHandle,
    submenu: &Submenu<tauri::Wry>,
    state: &totp::TotpState,
) {
    let entries = totp::menu_entries(state);
    let expected_ids: Vec<String> = entries
        .iter()
        .map(|(id, _)| format!("totp-item-{id}"))
        .collect();
    let existing = submenu.items().unwrap_or_default();
    let existing_ids: Vec<String> = existing
        .iter()
        .filter(|item| item.id().as_ref().starts_with("totp-item-"))
        .map(|item| item.id().as_ref().to_string())
        .collect();

    if existing_ids != expected_ids {
        rebuild_totp_menu(app, submenu, state);
        return;
    }

    for (id, label) in entries {
        let menu_id = format!("totp-item-{id}");
        if let Some(item) = existing
            .iter()
            .find(|item| item.id().as_ref() == menu_id)
            .and_then(|item| item.as_menuitem())
        {
            if let Err(error) = item.set_text(label) {
                eprintln!("更新状态栏 2FA 验证码失败：{error}");
            }
        }
    }
}

fn rebuild_totp_menu(
    app: &tauri::AppHandle,
    submenu: &Submenu<tauri::Wry>,
    state: &totp::TotpState,
) {
    let entries = totp::menu_entries(state);
    if let Ok(existing) = submenu.items() {
        for item in existing {
            let _ = submenu.remove(&item);
        }
    }

    let mut owned: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = Vec::new();
    if entries.is_empty() {
        if let Ok(item) = MenuItem::with_id(app, "totp-empty", "暂无账户", false, None::<&str>)
        {
            owned.push(Box::new(item));
        }
    } else {
        for (id, label) in entries {
            if let Ok(item) =
                MenuItem::with_id(app, format!("totp-item-{id}"), label, true, None::<&str>)
            {
                owned.push(Box::new(item));
            }
        }
    }
    if let Ok(separator) = PredefinedMenuItem::separator(app) {
        owned.push(Box::new(separator));
    }
    if let Ok(item) = MenuItem::with_id(app, "manage-totp", "管理 2FA 验证码…", true, None::<&str>)
    {
        owned.push(Box::new(item));
    }
    let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = owned.iter().map(|item| item.as_ref()).collect();
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
