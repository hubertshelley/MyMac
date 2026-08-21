use fontdue::{Font, FontSettings, LineMetrics};
use std::sync::OnceLock;
use tauri::image::Image;

use crate::config::StatusConfig;
use crate::system::SystemInfo;

/// 图标整体高度（2x，Tauri 会缩放到菜单栏标准 18pt 高）
const ICON_HEIGHT: u32 = 36;
const FONT_SIZE: f32 = 24.0;
const PADDING_X: i32 = 7;
const LOGO_SIZE: i32 = 32;
const METRIC_ICON_SIZE: i32 = 16;
const GAP: i32 = 9;
const ICON_TEXT_GAP: i32 = 4;

#[derive(Clone, Copy)]
enum Metric {
    Cpu,
    Memory,
    Disk,
}

enum Segment {
    Logo,
    Metric(Metric, String),
}

fn font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        let bytes = std::fs::read("/System/Library/Fonts/SFNS.ttf")
            .or_else(|_| std::fs::read("/System/Library/Fonts/Monaco.ttf"))
            .expect("无法加载系统字体");
        Font::from_bytes(bytes, FontSettings::default()).expect("字体解析失败")
    })
}

/// 检测系统是否处于深色模式
fn is_dark_mode() -> bool {
    std::process::Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim() == "Dark")
        .unwrap_or(false)
}

/// 加载并缩放应用 Logo（彩色）
fn logo_rgba() -> &'static (Vec<u8>, u32, u32) {
    static LOGO: OnceLock<(Vec<u8>, u32, u32)> = OnceLock::new();
    LOGO.get_or_init(|| {
        let bytes = include_bytes!("../icons/128x128.png");
        let img = image::load_from_memory(bytes).expect("Logo 解码失败");
        let img = img.resize_exact(
            LOGO_SIZE as u32,
            LOGO_SIZE as u32,
            image::imageops::FilterType::Lanczos3,
        );
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        (rgba.into_raw(), w, h)
    })
}

/// 根据配置与系统快照渲染状态栏图标（彩色 Logo + 单色指标，颜色跟随主题）
pub fn render_status_icon(info: &SystemInfo, config: &StatusConfig) -> Image<'static> {
    let dark = is_dark_mode();
    let color = if dark { 255u8 } else { 0u8 };

    let mut segments: Vec<Segment> = Vec::new();
    if config.show_logo {
        segments.push(Segment::Logo);
    }
    if config.show_cpu {
        segments.push(Segment::Metric(Metric::Cpu, format!("{:.0}%", info.cpu_usage)));
    }
    if config.show_memory {
        segments.push(Segment::Metric(
            Metric::Memory,
            format!("{:.0}%", info.memory_usage),
        ));
    }
    if config.show_disk {
        if let Some(d) = info.disks.first() {
            segments.push(Segment::Metric(Metric::Disk, format!("{:.0}%", d.usage)));
        }
    }
    if segments.is_empty() {
        segments.push(Segment::Metric(Metric::Cpu, "--".to_string()));
    }

    // 计算总宽度
    let mut width = PADDING_X * 2;
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            width += GAP;
        }
        width += match seg {
            Segment::Logo => LOGO_SIZE,
            Segment::Metric(_, text) => {
                METRIC_ICON_SIZE + ICON_TEXT_GAP + text_width(text) as i32
            }
        };
    }

    let mut buf = vec![0u8; (width as u32 * ICON_HEIGHT * 4) as usize];

    let mut x = PADDING_X;
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            x += GAP;
        }
        match seg {
            Segment::Logo => {
                draw_logo(&mut buf, width as u32, x);
                x += LOGO_SIZE;
            }
            Segment::Metric(metric, text) => {
                draw_metric_icon(&mut buf, width as u32, x, *metric, color);
                x += METRIC_ICON_SIZE + ICON_TEXT_GAP;
                draw_text(&mut buf, width as u32, x, text, color);
                x += text_width(text) as i32;
            }
        }
    }

    Image::new_owned(buf, width as u32, ICON_HEIGHT)
}

fn text_width(text: &str) -> f32 {
    let mut width = 0.0;
    for ch in text.chars() {
        let (metrics, _) = font().rasterize(ch, FONT_SIZE);
        width += metrics.advance_width;
    }
    width
}

fn set_px(buf: &mut [u8], width: u32, x: i32, y: i32, color: u8) {
    if x < 0 || y < 0 || x >= width as i32 || y >= ICON_HEIGHT as i32 {
        return;
    }
    let idx = ((y as u32 * width + x as u32) * 4) as usize;
    buf[idx] = color;
    buf[idx + 1] = color;
    buf[idx + 2] = color;
    buf[idx + 3] = 255;
}

/// 把彩色 Logo 叠加到画布指定位置（垂直居中）
fn draw_logo(buf: &mut [u8], width: u32, x: i32) {
    let (logo, lw, lh) = logo_rgba();
    let y = (ICON_HEIGHT as i32 - *lh as i32) / 2;
    for ly in 0..*lh as i32 {
        for lx in 0..*lw as i32 {
            let px = x + lx;
            let py = y + ly;
            if px < 0 || py < 0 || px >= width as i32 || py >= ICON_HEIGHT as i32 {
                continue;
            }
            let src = ((ly as u32 * *lw + lx as u32) * 4) as usize;
            let a = logo[src + 3];
            if a == 0 {
                continue;
            }
            let dst = ((py as u32 * width + px as u32) * 4) as usize;
            buf[dst] = logo[src];
            buf[dst + 1] = logo[src + 1];
            buf[dst + 2] = logo[src + 2];
            buf[dst + 3] = a;
        }
    }
}

/// 绘制指标小图标（单色几何图形）
fn draw_metric_icon(buf: &mut [u8], width: u32, ox: i32, metric: Metric, color: u8) {
    let size = METRIC_ICON_SIZE;
    let top = (ICON_HEIGHT as i32 - size) / 2;
    match metric {
        Metric::Cpu => {
            // 芯片：正方形轮廓 + 内部实心小方块
            let half = size / 2;
            let cx = ox + size / 2;
            let cy = top + size / 2;
            let border = 2;
            for dy in -half..=half {
                for dx in -half..=half {
                    let is_border = dx.abs() >= half - border || dy.abs() >= half - border;
                    if is_border {
                        set_px(buf, width, cx + dx, cy + dy, color);
                    }
                }
            }
            let inner = size / 4;
            for dy in -inner..=inner {
                for dx in -inner..=inner {
                    set_px(buf, width, cx + dx, cy + dy, color);
                }
            }
        }
        Metric::Memory => {
            // 内存条：三个横条
            let bar_h = 2;
            let gap = 2;
            for i in 0..3 {
                let by = top + i * (bar_h + gap);
                for dy in 0..bar_h {
                    for dx in 0..size {
                        set_px(buf, width, ox + dx, by + dy, color);
                    }
                }
            }
        }
        Metric::Disk => {
            // 硬盘：实心圆
            let cx = ox + size / 2;
            let cy = top + size / 2;
            let r = size / 2;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx * dx + dy * dy <= r * r {
                        set_px(buf, width, cx + dx, cy + dy, color);
                    }
                }
            }
        }
    }
}

fn draw_text(buf: &mut [u8], width: u32, x: i32, text: &str, color: u8) {
    let font = font();
    let line_metrics = font.vertical_line_metrics(FONT_SIZE).unwrap_or(LineMetrics {
        ascent: FONT_SIZE * 0.8,
        descent: -FONT_SIZE * 0.2,
        line_gap: 0.0,
        new_line_size: FONT_SIZE,
    });
    // 垂直居中：baseline = (H + ascent + descent) / 2（descent 为负）
    let baseline_y = (ICON_HEIGHT as f32 + line_metrics.ascent + line_metrics.descent) / 2.0;

    let mut pen_x = x as f32;
    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, FONT_SIZE);
        // ymin 是位图底部相对 baseline 的偏移，位图顶部 = baseline - ymin - height
        let ox = pen_x as i32 + metrics.xmin;
        let oy = baseline_y as i32 - metrics.ymin - metrics.height as i32;
        for gy in 0..metrics.height {
            for gx in 0..metrics.width {
                let cov = bitmap[gy * metrics.width + gx];
                if cov == 0 {
                    continue;
                }
                let px = ox + gx as i32;
                let py = oy + gy as i32;
                if px < 0 || py < 0 || px >= width as i32 || py >= ICON_HEIGHT as i32 {
                    continue;
                }
                let idx = ((py as u32 * width + px as u32) * 4) as usize;
                buf[idx] = color;
                buf[idx + 1] = color;
                buf[idx + 2] = color;
                buf[idx + 3] = cov;
            }
        }
        pen_x += metrics.advance_width;
    }
}
