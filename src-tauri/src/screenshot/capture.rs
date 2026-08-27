//! 屏幕捕获：显示器枚举、画面抓取与屏幕录制权限检测。

use core_graphics::display::{
    CGDisplay, CGDisplayCopyDisplayMode, CGDisplayModeGetPixelWidth, CGDisplayModeRelease,
};
use serde::Serialize;

/// 单个显示器的捕获信息（坐标为 CG 全局逻辑坐标，左上原点）
#[derive(Debug, Clone, Serialize)]
pub struct DisplayInfo {
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// 物理像素 / 逻辑单位
    pub scale: f64,
}

extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// 是否已授予屏幕录制权限
pub fn has_screen_capture_access() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// 触发系统授权弹窗（仅首次未授权时弹出）
pub fn request_screen_capture_access() -> bool {
    unsafe { CGRequestScreenCaptureAccess() }
}

/// 枚举所有活动显示器
pub fn active_displays() -> Vec<DisplayInfo> {
    let ids = CGDisplay::active_displays().unwrap_or_default();
    let mut result = Vec::with_capacity(ids.len());
    for id in ids {
        let display = CGDisplay::new(id);
        let bounds = display.bounds();
        if bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
            continue;
        }
        let scale = display_scale(id, bounds.size.width);
        result.push(DisplayInfo {
            id,
            x: bounds.origin.x,
            y: bounds.origin.y,
            width: bounds.size.width,
            height: bounds.size.height,
            scale,
        });
    }
    result
}

/// 计算物理像素 / 逻辑单位缩放比。
/// 注意 `CGDisplay::pixels_wide()` 在 Retina 屏上返回逻辑宽度，
/// 必须从显示模式读取真实像素宽度。
fn display_scale(display_id: u32, logical_width: f64) -> f64 {
    if logical_width <= 0.0 {
        return 1.0;
    }
    unsafe {
        let mode = CGDisplayCopyDisplayMode(display_id);
        if mode.is_null() {
            return 1.0;
        }
        let pixel_width = CGDisplayModeGetPixelWidth(mode) as f64;
        CGDisplayModeRelease(mode);
        (pixel_width / logical_width).round().max(1.0)
    }
}

/// 抓取指定显示器完整画面并编码为 PNG，返回 (PNG 字节, 物理宽, 物理高)
pub fn grab_display_png(display_id: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let display = CGDisplay::new(display_id);
    let cg_image = display
        .image()
        .ok_or_else(|| "抓取屏幕画面失败".to_string())?;
    let width = cg_image.width();
    let height = cg_image.height();
    let data = cg_image.data();
    let raw = data.bytes();
    let expected = width * height * 4;
    if raw.len() < expected {
        return Err("屏幕图像数据不完整".to_string());
    }

    // CoreGraphics 返回 BGRA 预乘字节序，转换为 RGBA
    let mut rgba = raw[..expected].to_vec();
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }

    let frame = image::RgbaImage::from_raw(width as u32, height as u32, rgba)
        .ok_or_else(|| "图像缓冲无效".to_string())?;
    let mut cursor = std::io::Cursor::new(Vec::new());
    frame
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| format!("PNG 编码失败：{e}"))?;
    Ok((cursor.into_inner(), width as u32, height as u32))
}
