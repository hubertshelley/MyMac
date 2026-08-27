//! 截图功能后端能力的集成验证（仅 macOS 有意义）：
//! 显示器枚举、屏幕抓取与 PNG 编码、窗口列表枚举。

#![cfg(target_os = "macos")]

use mymac_lib::screenshot::{capture, winlist};

#[test]
fn active_displays_returns_valid_geometry() {
    let displays = capture::active_displays();
    assert!(!displays.is_empty(), "应至少枚举到一个显示器");
    for display in &displays {
        assert!(
            display.width > 0.0 && display.height > 0.0,
            "显示器尺寸应为正"
        );
        assert!(display.scale >= 1.0, "缩放比应不小于 1");
    }
}

#[test]
fn grab_display_produces_decodable_png() {
    let displays = capture::active_displays();
    let first = displays
        .first()
        .cloned()
        .expect("至少需要一个显示器才能抓屏");

    let (png, width, height) = capture::grab_display_png(first.id).expect("抓屏应成功");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "应为合法 PNG 头");
    assert!(width > 0 && height > 0);

    // 物理像素应接近逻辑尺寸 × 缩放比
    let ratio_w = width as f64 / first.width;
    assert!(
        (ratio_w - first.scale).abs() < 0.5,
        "宽度比例 {ratio_w} 应接近缩放比 {}",
        first.scale
    );

    // PNG 可被 image crate 解码且尺寸一致
    let decoded = image::load_from_memory(&png).expect("PNG 应可解码");
    assert_eq!(decoded.width(), width);
    assert_eq!(decoded.height(), height);
}

#[test]
fn window_listing_returns_finite_rects() {
    let windows = winlist::list_on_screen_windows();
    for window in &windows {
        assert!(
            window.width >= 40.0 && window.height >= 40.0,
            "窗口尺寸不应小于过滤阈值"
        );
        assert!(
            window.x.is_finite() && window.y.is_finite(),
            "坐标应为有限数值"
        );
        assert!(window.window_id > 0, "窗口编号应为正");
    }
}
