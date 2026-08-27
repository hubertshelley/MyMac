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
    let grab = grab_display_raw(display_id)?;
    let rgba = bgra_to_rgba(&grab.raw, grab.width, grab.height, grab.bytes_per_row)?;
    let png = encode_png(rgba, grab.width, grab.height)?;
    Ok((png, grab.width as u32, grab.height as u32))
}

/// 抓屏的原始结果：未解码的 BGRA 数据与几何信息。
/// 抓取（CGDisplayCreateImage）必须在主线程执行，
/// 后续的像素转换与 PNG 编码可移至后台线程并行处理。
pub struct RawGrab {
    pub display_id: u32,
    pub raw: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub bytes_per_row: usize,
}

/// 抓取指定显示器的原始画面数据（仅可在主线程调用）
pub fn grab_display_raw(display_id: u32) -> Result<RawGrab, String> {
    let display = CGDisplay::new(display_id);
    let cg_image = display
        .image()
        .ok_or_else(|| "抓取屏幕画面失败".to_string())?;
    let width = cg_image.width();
    let height = cg_image.height();
    let bytes_per_row = cg_image.bytes_per_row();
    if cg_image.bits_per_pixel() != 32 || cg_image.bits_per_component() != 8 {
        return Err(format!(
            "不支持的屏幕像素格式：{}bpp/{}bpc",
            cg_image.bits_per_pixel(),
            cg_image.bits_per_component()
        ));
    }
    let image_data = cg_image.data();
    let raw = image_data.bytes().to_vec();
    Ok(RawGrab {
        display_id,
        raw,
        width,
        height,
        bytes_per_row,
    })
}

/// 将 RGBA 像素编码为 PNG（快速压缩档，显著降低编码耗时）
pub fn encode_png(rgba: Vec<u8>, width: usize, height: usize) -> Result<Vec<u8>, String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;

    let mut cursor = std::io::Cursor::new(Vec::new());
    let encoder = PngEncoder::new_with_quality(
        &mut cursor,
        CompressionType::Fast,
        FilterType::NoFilter,
    );
    encoder
        .write_image(
            &rgba,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("PNG 编码失败：{e}"))?;
    Ok(cursor.into_inner())
}

/// 从原始抓屏数据转换并编码 PNG（供后台线程调用）
pub fn grab_display_png_for_raw(grab: RawGrab) -> Result<(u32, Vec<u8>), String> {
    let rgba = bgra_to_rgba(&grab.raw, grab.width, grab.height, grab.bytes_per_row)?;
    let png = encode_png(rgba, grab.width, grab.height)?;
    Ok((grab.display_id, png))
}

/// 将 CoreGraphics 的 BGRA 像素数据转换为 RGBA。
/// 必须按 bytes_per_row 逐行拷贝：每行末尾可能存在对齐填充字节，
/// 直接按 width*height*4 切分整块缓冲会把填充混入像素流，导致图像撕裂。
fn bgra_to_rgba(
    raw: &[u8],
    width: usize,
    height: usize,
    bytes_per_row: usize,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("图像尺寸无效".to_string());
    }
    let line_bytes = width * 4;
    if bytes_per_row < line_bytes {
        return Err("行字节数小于像素宽度所需".to_string());
    }
    let mut rgba = Vec::with_capacity(line_bytes * height);
    for row in 0..height {
        let start = row * bytes_per_row;
        let end = start + line_bytes;
        if raw.len() < end {
            return Err("屏幕图像数据不完整".to_string());
        }
        rgba.extend_from_slice(&raw[start..end]);
    }
    // BGRA 字节序转 RGBA
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_to_rgba_handles_row_padding() {
        // 3 像素宽、2 行高，行跨度 16 字节（每行末尾 4 字节对齐填充）
        let mut raw = vec![0u8; 16 * 2];
        for row in 0..2 {
            let base = row * 16;
            raw[base..base + 12]
                .copy_from_slice(&[1, 2, 3, 255, 5, 6, 7, 255, 9, 10, 11, 255]);
        }
        let rgba = bgra_to_rgba(&raw, 3, 2, 16).expect("转换应成功");
        assert_eq!(rgba.len(), 3 * 2 * 4);
        // 首像素 BGRA(1,2,3,255) 转 RGBA 后为 (3,2,1,255)
        assert_eq!(&rgba[0..4], &[3, 2, 1, 255]);
        // 第二行从像素数据开始，填充字节未混入
        assert_eq!(&rgba[12..16], &[3, 2, 1, 255]);
        assert_eq!(&rgba[20..24], &[11, 10, 9, 255]);
    }

    #[test]
    fn bgra_to_rgba_rejects_invalid_input() {
        assert!(bgra_to_rgba(&[], 0, 0, 0).is_err());
        // 数据长度不足
        assert!(bgra_to_rgba(&vec![0u8; 10], 3, 2, 12).is_err());
        // 行跨度小于像素宽度所需
        assert!(bgra_to_rgba(&vec![0u8; 64], 4, 2, 8).is_err());
    }
}
