//! 托盘图标与菜单（tray-icon + muda）。必须在主线程创建。

use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::event::{UiEvent, UiSink};

const ID_SHOW: &str = "show";
const ID_SCREENSHOT: &str = "screenshot";
const ID_QUIT: &str = "quit";

pub fn setup() -> Result<TrayIcon, Box<dyn std::error::Error + Send + Sync>> {
    let show = MenuItem::with_id(ID_SHOW, "打开主面板", true, None);
    let screenshot = MenuItem::with_id(ID_SCREENSHOT, "截图（F1）", true, None);
    let quit = MenuItem::with_id(ID_QUIT, "退出 MyMac", true, None);
    let menu = Menu::new();
    menu.append_items(&[&screenshot, &show, &PredefinedMenuItem::separator(), &quit])?;

    let (rgba, w, h) = placeholder_icon();
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(true)
        .with_tooltip("MyMac 电脑管家")
        .with_icon(Icon::from_rgba(rgba, w, h)?)
        .build()
        .map_err(Into::into)
}

/// 把 muda / 托盘事件转成 `UiEvent`。事件 receiver 是全局的，开后台线程阻塞读取。
pub fn spawn_forwarder(sink: UiSink) {
    let menu_sink = sink.clone();
    std::thread::spawn(move || {
        while let Ok(event) = MenuEvent::receiver().recv() {
            let ui_event = match event.id().as_ref() {
                ID_SHOW => UiEvent::ShowMainWindow,
                ID_SCREENSHOT => UiEvent::StartScreenshot,
                ID_QUIT => UiEvent::Quit,
                _ => continue,
            };
            menu_sink.send(ui_event);
        }
    });
    // 与原 Tauri 版一致：托盘左键抬起既弹菜单也唤回主窗口
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

/// 占位图标（M2 起由 `mymac_core::status::render_status_icon` 每 2 秒刷新）
fn placeholder_icon() -> (Vec<u8>, u32, u32) {
    let (w, h) = (32u32, 32u32);
    let rgba = std::iter::repeat_n([40u8, 120, 220, 255], (w * h) as usize)
        .flatten()
        .collect();
    (rgba, w, h)
}
