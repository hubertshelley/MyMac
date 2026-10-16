//! macOS 专属：Dock 图标显隐（对应 Tauri 的 `set_dock_visibility`）。
//!
//! 迁移对照：Tauri `AppHandle::set_dock_visibility(false)` 内部就是
//! `NSApplication.setActivationPolicy(.accessory)`；恢复传 `.regular`。

use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSImage};
use objc2_foundation::NSData;

/// 显式设置 Dock / 应用图标。Info.plist 的 icns 在部分启动方式下不生效，
/// 统一在启动时用打包好的 icon.png 设置一次。
pub fn install_app_icon() {
    const PNG: &[u8] = include_bytes!("../assets/icon.png");
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let data = NSData::with_bytes(PNG);
    let image = NSImage::initWithData(NSImage::alloc(), &data);
    if let Some(image) = image {
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
}

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
