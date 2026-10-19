//! macOS 专属：Dock 图标显隐（对应 Tauri 的 `set_dock_visibility`）。
//!
//! 迁移对照：Tauri `AppHandle::set_dock_visibility(false)` 内部就是
//! `NSApplication.setActivationPolicy(.accessory)`；恢复传 `.regular`。

use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSImage};
use objc2_foundation::NSData;

/// 把指定标题前缀的窗口移出屏幕（orderOut），返回其中是否有正在显示的窗口。
/// 截图重触发时先移走旧覆盖层，避免把覆盖层自己抓进冻结背景。
pub fn order_out_titled_prefix(title_prefix: &str) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    let mut visible = false;
    for window in app.windows() {
        if window.title().to_string().starts_with(title_prefix) {
            visible |= window.isVisible();
            window.orderOut(None);
        }
    }
    visible
}

/// 激活本应用。托盘常驻（Accessory）时不激活的话，覆盖层和保存面板拿不到键盘焦点，
/// Esc / Enter 都不生效。
#[allow(deprecated)] // activateIgnoringOtherApps 在 macOS 14 标记弃用，但仍是 Accessory 应用唯一可靠的方式
pub fn activate_app() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
}

/// 交还焦点给之前的前台应用（截图结束且主窗口未显示时调用）。
pub fn deactivate_app() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    NSApplication::sharedApplication(mtm).deactivate();
}

/// 主窗口（标题 MyMac）当前是否可见。
pub fn main_window_visible() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    for window in app.windows() {
        if window.title().to_string() == "MyMac" && window.isVisible() {
            return true;
        }
    }
    false
}

/// 直接移出 / 恢复主窗口（截图前移出，避免把主窗口截进去；结束后恢复）。
/// 走 AppKit 而不是 egui 命令：egui 命令要到下一帧才生效，而抓屏必须马上进行。
pub fn order_main_window(front: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    for window in app.windows() {
        if window.title().to_string() == "MyMac" {
            if front {
                window.orderFrontRegardless();
            } else {
                window.orderOut(None);
            }
        }
    }
}

/// 把指定标题前缀的窗口提到最高层级（screenSaver 级，覆盖 Dock）。
/// 截图覆盖层用：普通 always-on-top 会被 Dock 盖住。
pub fn raise_windows_titled_prefix(title_prefix: &str) {
    set_level_titled_prefix(title_prefix, 1000); // kCGScreenSaverWindowLevel
}

/// 设置指定标题前缀窗口的层级。保存对话框弹出前先把覆盖层降到普通层级（0），
/// 否则 NSSavePanel（模态面板层级 8）会被 screenSaver 级覆盖层挡住。
pub fn set_level_titled_prefix(title_prefix: &str, level: isize) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    for window in app.windows() {
        if window.title().to_string().starts_with(title_prefix) {
            window.setLevel(level);
        }
    }
}

/// 贴图窗口真正「始终置顶」：egui 的 always_on_top 只是 Floating 级（3），
/// 会被 Dock、其他应用浮动面板盖住，且窗口只属于创建时的桌面空间。
/// 这里提到 NSStatusWindowLevel（25，与旧版一致，低于截图覆盖层 1000），
/// 并加入所有桌面空间、允许叠在全屏应用上、不进 ⌘Tab / 窗口循环。
/// 只在值不同时写入，可每帧调用。
pub fn pin_windows_titled_prefix(title_prefix: &str) {
    use objc2_app_kit::NSWindowCollectionBehavior as B;
    const NS_STATUS_WINDOW_LEVEL: isize = 25;
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let wanted = B::CanJoinAllSpaces | B::FullScreenAuxiliary | B::IgnoresCycle;
    let app = NSApplication::sharedApplication(mtm);
    for window in app.windows() {
        if !window.title().to_string().starts_with(title_prefix) {
            continue;
        }
        if window.level() != NS_STATUS_WINDOW_LEVEL {
            window.setLevel(NS_STATUS_WINDOW_LEVEL);
            crate::screenshot::diag(format!(
                "贴图窗口置顶：{} level={} behavior={:#x}",
                window.title(),
                window.level(),
                (window.collectionBehavior() | wanted).0
            ));
        }
        let behavior = window.collectionBehavior();
        if !behavior.contains(wanted) {
            window.setCollectionBehavior(behavior | wanted);
        }
    }
}

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

/// 状态栏按钮当前呈现的是否为深色外观。
///
/// 不能用 `defaults read -g AppleInterfaceStyle`（系统深色模式）判断：macOS 菜单栏会随壁纸
/// 呈现浅色或深色，深色模式 + 浅色壁纸时菜单栏是浅色的，按深色模式画白字就看不清。
/// 状态栏按钮的 effectiveAppearance 才是菜单栏实际的外观（Aqua / DarkAqua 及其 Vibrant 变体）。
/// 取不到按钮（未在主线程或状态项尚未创建）时返回 None，由调用方保留上一次的结果。
pub fn status_item_is_dark(item: &objc2_app_kit::NSStatusItem) -> Option<bool> {
    use objc2_app_kit::{
        NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    };
    use objc2_foundation::NSArray;
    let mtm = MainThreadMarker::new()?;
    let button = item.button(mtm)?;
    let appearance = button.effectiveAppearance();
    let candidates =
        unsafe { NSArray::from_slice(&[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]) };
    let best = appearance.bestMatchFromAppearancesWithNames(&candidates)?;
    Some(&*best == unsafe { NSAppearanceNameDarkAqua })
}
