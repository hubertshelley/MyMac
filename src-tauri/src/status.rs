use fontdue::{Font, FontSettings, LineMetrics};
use std::sync::OnceLock;
use tauri::image::Image;

use crate::config::StatusConfig;
use crate::system::SystemInfo;

/// 图标整体高度（2x，Tauri 会缩放到菜单栏标准 18pt 高）
const ICON_HEIGHT: u32 = 36;
const FONT_SIZE: f32 = 24.0;
const NETWORK_FONT_SIZE: f32 = 15.0;
const ICON_FONT_SIZE: f32 = 20.0;
const PADDING_X: i32 = 7;
const LOGO_SIZE: i32 = 32;
const GAP: i32 = 9;
const ICON_TEXT_GAP: i32 = 4;

/// Material Icons 码点
const ICON_CPU: u32 = 0xe30d; // developer_board
const ICON_MEMORY: u32 = 0xe322; // memory
const ICON_DISK: u32 = 0xe1db; // storage
const ICON_NETWORK: u32 = 0xe8d5; // swap_vert

#[derive(Clone, Copy)]
enum Metric {
    Cpu,
    Memory,
    Disk,
}

enum Segment {
    Logo,
    Metric(Metric, String),
    Network { down: String, up: String },
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

fn icon_font() -> &'static Font {
    static ICON_FONT: OnceLock<Font> = OnceLock::new();
    ICON_FONT.get_or_init(|| {
        let bytes = include_bytes!("../fonts/MaterialIcons-Regular.ttf");
        Font::from_bytes(&bytes[..], FontSettings::default()).expect("图标字体解析失败")
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

/// 将字节/秒速率格式化为简洁字符串
pub fn format_rate(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1_000_000.0 {
        format!("{:.1}M", bytes_per_sec / 1_000_000.0)
    } else if bytes_per_sec >= 1_000.0 {
        format!("{:.0}K", bytes_per_sec / 1_000.0)
    } else if bytes_per_sec >= 1.0 {
        format!("{bytes_per_sec:.0}")
    } else {
        "0".to_string()
    }
}

/// 根据配置与系统快照渲染状态栏图标（彩色 Logo + 字体图标指标）
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
    if config.show_network {
        segments.push(Segment::Network {
            down: format_rate(info.net_down_rate),
            up: format_rate(info.net_up_rate),
        });
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
                ICON_FONT_SIZE as i32 + ICON_TEXT_GAP + text_width(text) as i32
            }
            Segment::Network { down, up } => {
                let down_text = format!("↓{down}");
                let up_text = format!("↑{up}");
                ICON_FONT_SIZE as i32
                    + ICON_TEXT_GAP
                    + network_text_width(&down_text).max(network_text_width(&up_text)) as i32
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
                let code = match metric {
                    Metric::Cpu => ICON_CPU,
                    Metric::Memory => ICON_MEMORY,
                    Metric::Disk => ICON_DISK,
                };
                draw_icon_char(&mut buf, width as u32, x, code, color);
                x += ICON_FONT_SIZE as i32 + ICON_TEXT_GAP;
                draw_text(&mut buf, width as u32, x, text, color);
                x += text_width(text) as i32;
            }
            Segment::Network { down, up } => {
                draw_icon_char(&mut buf, width as u32, x, ICON_NETWORK, color);
                x += ICON_FONT_SIZE as i32 + ICON_TEXT_GAP;
                let down_text = format!("↓{down}");
                let up_text = format!("↑{up}");
                let text_width = network_text_width(&down_text).max(network_text_width(&up_text));
                draw_network_text(&mut buf, width as u32, x, &up_text, &down_text, color);
                x += text_width as i32;
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

fn network_text_width(text: &str) -> f32 {
    let mut width = 0.0;
    for ch in text.chars() {
        let (metrics, _) = font().rasterize(ch, NETWORK_FONT_SIZE);
        width += metrics.advance_width;
    }
    width
}

fn draw_network_text(
    buf: &mut [u8],
    width: u32,
    x: i32,
    up_text: &str,
    down_text: &str,
    color: u8,
) {
    // 36px 画布分成上下两行，各占 18px。
    draw_text_at(buf, width, x, up_text, NETWORK_FONT_SIZE, 0.0, 18.0, color);
    draw_text_at(
        buf,
        width,
        x,
        down_text,
        NETWORK_FONT_SIZE,
        18.0,
        ICON_HEIGHT as f32,
        color,
    );
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

/// 渲染一个图标字体字形（垂直居中）
fn draw_icon_char(buf: &mut [u8], width: u32, x: i32, code_point: u32, color: u8) {
    let ch = char::from_u32(code_point).expect("无效图标码点");
    let (metrics, bitmap) = icon_font().rasterize(ch, ICON_FONT_SIZE);
    let ox = x + metrics.xmin;
    let oy = (ICON_HEIGHT as i32 - metrics.height as i32) / 2;
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
}

fn draw_text(buf: &mut [u8], width: u32, x: i32, text: &str, color: u8) {
    draw_text_at(
        buf,
        width,
        x,
        text,
        FONT_SIZE,
        0.0,
        ICON_HEIGHT as f32,
        color,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_text_at(
    buf: &mut [u8],
    width: u32,
    x: i32,
    text: &str,
    font_size: f32,
    top: f32,
    bottom: f32,
    color: u8,
) {
    let font = font();
    let line_metrics = font.vertical_line_metrics(font_size).unwrap_or(LineMetrics {
        ascent: font_size * 0.8,
        descent: -font_size * 0.2,
        line_gap: 0.0,
        new_line_size: font_size,
    });
    let baseline_y =
        (top + bottom + line_metrics.ascent + line_metrics.descent) / 2.0;

    let mut pen_x = x as f32;
    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, font_size);
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
