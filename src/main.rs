//! MyMac 电脑管家（egui 版）。
//!
//! 关键机制（M0 已验证）：
//! - eframe 0.34 起 `App` 拆成 `logic` / `ui`：窗口隐藏时 `ui` 不跑，被
//!   `request_repaint_of(ROOT)` 唤醒的 `logic` 仍会执行——托盘、F1 事件在 `logic` 里消费。
//! - 托盘与全局快捷键必须在主线程创建，菜单修改也只能在主线程，所以后台线程只采样
//!   并发 `UiEvent`，由 `logic` 更新托盘。

mod app;
mod event;
mod fmt;
mod hotkey;
mod macos;
mod nav;
mod screenshot;
mod state;
mod task;
mod theme;
mod tray;
mod views;
mod widgets;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

use crate::event::{SharedCtx, UiEvent, UiSink};
use crate::state::Shared;

fn main() -> eframe::Result {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();
    // panic 信息写入诊断日志（.app 方式启动时 stderr 不可见，崩溃只表现为闪退）
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        let backtrace = std::backtrace::Backtrace::force_capture();
        crate::screenshot::diag(format!(
            "PANIC at {location}: {info}\nbacktrace:\n{backtrace}"
        ));
        eprintln!("PANIC at {location}: {info}");
    }));
    macos::install_app_icon();

    let (tx, rx) = mpsc::channel();
    let shared_ctx: SharedCtx = Arc::new(Mutex::new(None));
    let sink = UiSink::new(tx, shared_ctx.clone());
    let shared = Shared::load();

    // 托盘、快捷键对象必须存活到 run_native 返回，否则图标和热键会被销毁。
    let tray = tray::Tray::new(&shared).map_err(eframe::Error::AppCreation)?;
    let _hotkey = hotkey::setup().map_err(|e| eframe::Error::AppCreation(Box::new(e)))?;
    tray::spawn_forwarder(sink.clone(), shared.clone());
    hotkey::spawn_forwarder(sink.clone());
    spawn_sampler(sink.clone(), shared.clone());
    spawn_clipboard_watcher(sink.clone(), shared.clone());

    let window_icon = window_icon();
    eframe::run_native(
        "MyMac",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("MyMac")
                .with_inner_size([960.0, 640.0])
                .with_min_inner_size([820.0, 560.0])
                .with_icon(window_icon),
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx, theme::Appearance::default());
            *shared_ctx.lock().unwrap() = Some(cc.egui_ctx.clone());
            Ok(Box::new(app::MyMacApp::new(rx, sink, shared, tray)))
        }),
    )
}

/// 主窗口图标（复用打包好的 icon.png）。
fn window_icon() -> egui::IconData {
    const PNG: &[u8] = include_bytes!("../assets/icon.png");
    match image::load_from_memory(PNG) {
        Ok(image) => {
            let rgba = image.to_rgba8();
            egui::IconData {
                width: rgba.width(),
                height: rgba.height(),
                rgba: rgba.into_raw(),
            }
        }
        Err(error) => {
            log::warn!("加载窗口图标失败：{error}");
            egui::IconData::default()
        }
    }
}

/// 每 2 秒采样一次系统信息，并通知主线程刷新托盘与资源监控页。
fn spawn_sampler(sink: UiSink, shared: Shared) {
    std::thread::spawn(move || {
        loop {
            shared.sample();
            sink.send(UiEvent::SystemSampled);
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// 监听剪贴板变化，写入历史并通知主线程重建托盘子菜单。
fn spawn_clipboard_watcher(sink: UiSink, shared: Shared) {
    std::thread::spawn(move || {
        mymac_core::clipboard::watch_clipboard(&shared.clipboard, || {
            sink.send(UiEvent::ClipboardChanged);
        });
    });
}
