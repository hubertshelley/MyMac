//! 托盘图标与菜单（tray-icon + muda）。
//!
//! muda / tray-icon 在 macOS 上要求菜单与图标在**主线程**创建和修改，所以后台线程
//! 只负责采样，把结果通过 `TrayCommand` 发回主线程，由 `App::logic` 应用到菜单。

use muda::{IconMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use mymac_core::clipboard::{self, ClipItem};
use mymac_core::status::{self, RgbaIcon};
use mymac_core::system::SystemInfo;
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::event::{UiEvent, UiSink};
use crate::nav::Page;
use crate::state::Shared;

const ID_SHOW: &str = "show";
const ID_SCREENSHOT: &str = "screenshot";
const ID_QUIT: &str = "quit";
const ID_MANAGE_TOTP: &str = "manage-totp";
const ID_CLEAR_CLIP: &str = "clear-clip";
const PREFIX_CLIP: &str = "clip-item-";
const PREFIX_TOTP: &str = "totp-item-";

/// 托盘菜单：持有各菜单项句柄，供刷新时修改文字。仅限主线程使用。
pub struct Tray {
    icon: TrayIcon,
    cpu: MenuItem,
    mem: MenuItem,
    net: MenuItem,
    clip_menu: Submenu,
    totp_menu: Submenu,
}

impl Tray {
    pub fn new(shared: &Shared) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let cpu = MenuItem::with_id("cpu", "CPU：--", false, None);
        let mem = MenuItem::with_id("mem", "内存：--", false, None);
        let net = MenuItem::with_id("net", "网络：--", false, None);
        let show = MenuItem::with_id(ID_SHOW, "打开主面板", true, None);
        let screenshot = MenuItem::with_id(ID_SCREENSHOT, "截图（F1）", true, None);
        let quit = MenuItem::with_id(ID_QUIT, "退出 MyMac", true, None);
        let clip_menu = Submenu::with_id("clip-history", "粘贴板历史", true);
        let totp_menu = Submenu::with_id("totp-codes", "2FA 验证码", true);

        let menu = Menu::new();
        menu.append_items(&[
            &cpu,
            &mem,
            &net,
            &PredefinedMenuItem::separator(),
            &clip_menu,
            &totp_menu,
            &PredefinedMenuItem::separator(),
            &screenshot,
            &show,
            &quit,
        ])?;

        let initial = status::render_status_icon(
            &shared.monitor.lock().unwrap().snapshot(),
            &shared.config.lock().unwrap(),
        );
        let icon = TrayIconBuilder::new()
            .with_id("main-tray")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(true)
            .with_tooltip("MyMac 电脑管家")
            .with_icon(to_icon(initial)?)
            .build()?;

        let tray = Self {
            icon,
            cpu,
            mem,
            net,
            clip_menu,
            totp_menu,
        };
        tray.rebuild_clip_menu(shared);
        tray.rebuild_totp_menu(shared);
        Ok(tray)
    }

    /// 每 2 秒一次：实时信息、状态栏图标、2FA 验证码。
    pub fn refresh(&self, info: &SystemInfo, shared: &Shared) {
        self.cpu.set_text(format!("CPU：{:.1}%", info.cpu_usage));
        self.mem.set_text(format!(
            "内存：{:.1}%（{} / {}）",
            info.memory_usage,
            crate::fmt::bytes(info.memory_used),
            crate::fmt::bytes(info.memory_total)
        ));
        self.net.set_text(format!(
            "网络：↓ {} · ↑ {}",
            status::format_rate(info.net_down_rate),
            status::format_rate(info.net_up_rate)
        ));
        let config = shared.config.lock().unwrap().clone();
        if let Ok(icon) = to_icon(status::render_status_icon(info, &config)) {
            let _ = self.icon.set_icon(Some(icon));
        }
        self.update_totp_menu(shared);
    }

    /// 重建「粘贴板历史」子菜单。先克隆数据、释放锁，再操作菜单。
    pub fn rebuild_clip_menu(&self, shared: &Shared) {
        let entries: Vec<(String, String, Option<String>)> = {
            let items = shared.clipboard.items.lock().unwrap();
            items
                .iter()
                .take(clipboard::TRAY_SHOW_ITEMS)
                .map(|i: &ClipItem| (i.id.clone(), clipboard::menu_label(i), i.thumb_file.clone()))
                .collect()
        };
        clear_submenu(&self.clip_menu);
        if entries.is_empty() {
            let _ =
                self.clip_menu
                    .append(&MenuItem::with_id("clip-empty", "暂无记录", false, None));
            return;
        }
        for (id, label, thumb) in entries {
            let menu_id = format!("{PREFIX_CLIP}{id}");
            let icon = thumb
                .as_deref()
                .and_then(clipboard::load_thumb_image)
                .and_then(|(rgba, w, h)| muda::Icon::from_rgba(rgba, w, h).ok());
            match icon {
                // 图片记录：带缩略图图标（macOS 自动缩放到 18pt 高）
                Some(icon) => {
                    let _ = self.clip_menu.append(&IconMenuItem::with_id(
                        menu_id,
                        label,
                        true,
                        Some(icon),
                        None,
                    ));
                }
                None => {
                    let _ = self
                        .clip_menu
                        .append(&MenuItem::with_id(menu_id, label, true, None));
                }
            }
        }
        let _ = self.clip_menu.append(&PredefinedMenuItem::separator());
        let _ = self.clip_menu.append(&MenuItem::with_id(
            ID_CLEAR_CLIP,
            "清空粘贴板历史",
            true,
            None,
        ));
    }

    /// 2FA 子菜单：账户列表没变就只改文字，变了才重建。
    fn update_totp_menu(&self, shared: &Shared) {
        let entries = mymac_core::totp::menu_entries(&shared.totp);
        let expected: Vec<String> = entries
            .iter()
            .map(|(id, _)| format!("{PREFIX_TOTP}{id}"))
            .collect();
        let existing = self.totp_menu.items();
        let existing_ids: Vec<String> = existing
            .iter()
            .filter(|item| item.id().as_ref().starts_with(PREFIX_TOTP))
            .map(|item| item.id().as_ref().to_string())
            .collect();
        if existing_ids != expected {
            self.rebuild_totp_menu(shared);
            return;
        }
        for (id, label) in entries {
            let menu_id = format!("{PREFIX_TOTP}{id}");
            if let Some(item) = existing
                .iter()
                .find(|item| item.id().as_ref() == menu_id)
                .and_then(|item| item.as_menuitem())
            {
                item.set_text(label);
            }
        }
    }

    pub fn rebuild_totp_menu(&self, shared: &Shared) {
        let entries = mymac_core::totp::menu_entries(&shared.totp);
        clear_submenu(&self.totp_menu);
        if entries.is_empty() {
            let _ =
                self.totp_menu
                    .append(&MenuItem::with_id("totp-empty", "暂无账户", false, None));
        } else {
            for (id, label) in entries {
                let _ = self.totp_menu.append(&MenuItem::with_id(
                    format!("{PREFIX_TOTP}{id}"),
                    label,
                    true,
                    None,
                ));
            }
        }
        let _ = self.totp_menu.append(&PredefinedMenuItem::separator());
        let _ = self.totp_menu.append(&MenuItem::with_id(
            ID_MANAGE_TOTP,
            "管理 2FA 验证码…",
            true,
            None,
        ));
    }
}

fn clear_submenu(submenu: &Submenu) {
    use muda::MenuItemKind;
    for item in submenu.items() {
        let _ = match &item {
            MenuItemKind::MenuItem(i) => submenu.remove(i),
            MenuItemKind::Submenu(i) => submenu.remove(i),
            MenuItemKind::Predefined(i) => submenu.remove(i),
            MenuItemKind::Check(i) => submenu.remove(i),
            MenuItemKind::Icon(i) => submenu.remove(i),
        };
    }
}

fn to_icon(icon: RgbaIcon) -> Result<Icon, tray_icon::BadIcon> {
    Icon::from_rgba(icon.rgba, icon.width, icon.height)
}

/// 把 muda 菜单事件与托盘点击转成 `UiEvent`。事件 receiver 是全局的，用后台线程阻塞读取；
/// 粘贴板、2FA 的复制涉及剪贴板读写，也在这里的线程里做，避免阻塞主线程。
pub fn spawn_forwarder(sink: UiSink, shared: Shared) {
    let menu_sink = sink.clone();
    std::thread::spawn(move || {
        while let Ok(event) = MenuEvent::receiver().recv() {
            let id = event.id().as_ref();
            match id {
                ID_SHOW => menu_sink.send(UiEvent::ShowMainWindow),
                ID_SCREENSHOT => menu_sink.send(UiEvent::StartScreenshot),
                ID_QUIT => menu_sink.send(UiEvent::Quit),
                ID_MANAGE_TOTP => {
                    menu_sink.send(UiEvent::ShowMainWindow);
                    menu_sink.send(UiEvent::Navigate(Page::Totp));
                }
                ID_CLEAR_CLIP => {
                    let _ = clipboard::clear_history(&shared.clipboard);
                    menu_sink.send(UiEvent::ClipboardChanged);
                }
                _ if id.starts_with(PREFIX_CLIP) => {
                    let clip_id = id.trim_start_matches(PREFIX_CLIP);
                    let _ = clipboard::copy_to_clipboard_and_top(&shared.clipboard, clip_id);
                    menu_sink.send(UiEvent::ClipboardChanged);
                }
                _ if id.starts_with(PREFIX_TOTP) => {
                    let account_id = id.trim_start_matches(PREFIX_TOTP).to_string();
                    let result =
                        mymac_core::totp::copy_code(&shared.totp, &shared.clipboard, &account_id)
                            .map(|_| account_id);
                    if let Err(error) = &result {
                        log::error!("状态栏复制 2FA 验证码失败：{error}");
                    }
                    menu_sink.send(UiEvent::TotpCopied(result));
                }
                _ => {}
            }
        }
    });
    // 与旧版一致：托盘左键抬起既弹菜单也唤回主窗口
    std::thread::spawn(move || {
        while let Ok(event) = TrayIconEvent::receiver().recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                sink.send(UiEvent::ShowMainWindow);
            }
        }
    });
}
