//! macOS 专属：Dock 图标显隐（对应 Tauri 的 `set_dock_visibility`）。
//!
//! 迁移对照：Tauri `AppHandle::set_dock_visibility(false)` 内部就是
//! `NSApplication.setActivationPolicy(.accessory)`；恢复传 `.regular`。

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

/// 显隐 Dock 图标。必须在主线程调用（eframe 的 update 回调即主线程）。
pub fn set_dock_visible(visible: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("set_dock_visible 未在主线程调用，已忽略");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    app.setActivationPolicy(policy);
}
