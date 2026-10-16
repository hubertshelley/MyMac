//! MyMac 电脑管家（egui 版）。
//!
//! 关键机制（M0 已验证）：
//! - eframe 0.34 起 `App` 拆成 `logic` / `ui`：窗口隐藏时 `ui` 不跑，被
//!   `request_repaint_of(ROOT)` 唤醒的 `logic` 仍会执行——托盘、F1 事件在 `logic` 里消费。
//! - 托盘与全局快捷键必须在主线程创建，且要在事件循环启动前完成注册。

mod app;
mod event;
mod hotkey;
mod macos;
mod theme;
mod tray;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::event::{SharedCtx, UiSink};

fn main() -> eframe::Result {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();

    let (tx, rx) = mpsc::channel();
    let shared_ctx: SharedCtx = Arc::new(Mutex::new(None));
    let sink = UiSink::new(tx, shared_ctx.clone());

    // 托盘、快捷键对象必须存活到 run_native 返回，否则图标和热键会被销毁。
    let _tray = tray::setup().map_err(eframe::Error::AppCreation)?;
    let _hotkey = hotkey::setup().map_err(|e| eframe::Error::AppCreation(Box::new(e)))?;
    tray::spawn_forwarder(sink.clone());
    hotkey::spawn_forwarder(sink.clone());

    eframe::run_native(
        "MyMac",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("MyMac")
                .with_inner_size([960.0, 640.0])
                .with_min_inner_size([820.0, 560.0]),
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx, theme::Appearance::default());
            *shared_ctx.lock().unwrap() = Some(cc.egui_ctx.clone());
            Ok(Box::new(app::MyMacApp::new(rx, sink)))
        }),
    )
}
