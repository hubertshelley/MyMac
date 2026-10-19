use fontdue::{Font, FontSettings, LineMetrics};
use std::sync::OnceLock;

use crate::config::StatusConfig;
use crate::system::SystemInfo;

/// 图标整体高度（2x，托盘会缩放到菜单栏标准 18pt 高）
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
        let bytes = include_bytes!("../assets/MaterialIcons-Regular.ttf");
        Font::from_bytes(&bytes[..], FontSettings::default()).expect("图标字体解析失败")
    })
}

/// 加载并缩放应用 Logo（彩色）
fn logo_rgba() -> &'static (Vec<u8>, u32, u32) {
    static LOGO: OnceLock<(Vec<u8>, u32, u32)> = OnceLock::new();
    LOGO.get_or_init(|| {
        let bytes = include_bytes!("../assets/logo-128.png");
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

/// 将字节/秒速率格式化为简洁字符串，最长 5 个字符（如 `99.9M`）。
///
/// 阈值取 x99.5 而不是整千：否则 999.6 会四舍五入成 `1000`、999.6K 成 `1000K`，
/// 位数超出 [`NETWORK_TEMPLATES`] 的预留宽度。
pub fn format_rate(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 99_950_000_000.0 {
        format!("{:.0}G", bytes_per_sec / 1_000_000_000.0)
    } else if bytes_per_sec >= 999_500_000.0 {
        format!("{:.1}G", bytes_per_sec / 1_000_000_000.0)
    } else if bytes_per_sec >= 99_950_000.0 {
        format!("{:.0}M", bytes_per_sec / 1_000_000.0)
    } else if bytes_per_sec >= 999_500.0 {
        format!("{:.1}M", bytes_per_sec / 1_000_000.0)
    } else if bytes_per_sec >= 999.5 {
        format!("{:.0}K", bytes_per_sec / 1_000.0)
    } else if bytes_per_sec >= 1.0 {
        format!("{bytes_per_sec:.0}")
    } else {
        "0".to_string()
    }
}

/// 网络速率可能出现的最宽形态（数字按等宽计，用 8 占位）。网络段宽度按其中最宽者固定，
/// 速率变化时状态栏图标宽度不变，避免整个状态项左右跳动。
const NETWORK_TEMPLATES: [&str; 6] = ["↓888", "↓888K", "↓88.8M", "↓888M", "↓88.8G", "↓888G"];

/// RGBA 位图：宽、高与像素数据，由 UI 层转为托盘图标或纹理。
pub struct RgbaIcon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// 根据配置与系统快照渲染状态栏图标（彩色 Logo + 字体图标指标）。
///
/// `dark_menu_bar` 指菜单栏本身是否为深色外观，不是系统的深色模式：macOS 菜单栏会随壁纸
/// 呈现浅色或深色，深色模式下配浅色壁纸时菜单栏是浅色的，需画深色文字。由 UI 层读取
/// 状态栏按钮的 effectiveAppearance 传入。
pub fn render_status_icon(
    info: &SystemInfo,
    config: &StatusConfig,
    dark_menu_bar: bool,
) -> RgbaIcon {
    let color = if dark_menu_bar { 255u8 } else { 0u8 };

    let mut segments: Vec<Segment> = Vec::new();
    if config.show_logo {
        segments.push(Segment::Logo);
    }
    if config.show_cpu {
        segments.push(Segment::Metric(
            Metric::Cpu,
            format!("{:.0}%", info.cpu_usage),
        ));
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
            Segment::Network { .. } => {
                ICON_FONT_SIZE as i32 + ICON_TEXT_GAP + network_slot_width() as i32
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
                draw_network_text(&mut buf, width as u32, x, &up_text, &down_text, color);
                x += network_slot_width() as i32;
            }
        }
    }

    RgbaIcon {
        rgba: buf,
        width: width as u32,
        height: ICON_HEIGHT,
    }
}

fn text_width(text: &str) -> f32 {
    let mut width = 0.0;
    for ch in text.chars() {
        let (metrics, _) = font().rasterize(ch, FONT_SIZE);
        width += metrics.advance_width;
    }
    width
}

/// 网络文字里数字的统一步进：取 0～9 中最宽的字形。系统字体的数字默认是比例宽度
/// （`1` 比 `8` 窄），fontdue 不支持 OpenType 的 tnum 特性，只能自己按等宽排。
fn network_digit_advance() -> f32 {
    static ADVANCE: OnceLock<f32> = OnceLock::new();
    *ADVANCE.get_or_init(|| {
        ('0'..='9')
            .map(|ch| font().metrics(ch, NETWORK_FONT_SIZE).advance_width)
            .fold(0.0, f32::max)
    })
}

/// 字符在网络文字中的步进：数字等宽，其余按字形本身。
fn network_advance(ch: char) -> f32 {
    if ch.is_ascii_digit() {
        network_digit_advance()
    } else {
        font().metrics(ch, NETWORK_FONT_SIZE).advance_width
    }
}

fn network_text_width(text: &str) -> f32 {
    text.chars().map(network_advance).sum()
}

/// 网络文字区的固定宽度：所有可能形态中最宽的一种（↑ 与 ↓ 宽度不一定相同，一并计入）。
fn network_slot_width() -> f32 {
    static WIDTH: OnceLock<f32> = OnceLock::new();
    *WIDTH.get_or_init(|| {
        NETWORK_TEMPLATES
            .iter()
            .flat_map(|t| [t.to_string(), t.replacen('↓', "↑", 1)])
            .map(|t| network_text_width(&t))
            .fold(0.0, f32::max)
            .ceil()
    })
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
    draw_text_at(
        buf,
        width,
        x,
        up_text,
        NETWORK_FONT_SIZE,
        0.0,
        18.0,
        color,
        true,
    );
    draw_text_at(
        buf,
        width,
        x,
        down_text,
        NETWORK_FONT_SIZE,
        18.0,
        ICON_HEIGHT as f32,
        color,
        true,
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
        false,
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
    tabular: bool,
) {
    let font = font();
    let line_metrics = font
        .vertical_line_metrics(font_size)
        .unwrap_or(LineMetrics {
            ascent: font_size * 0.8,
            descent: -font_size * 0.2,
            line_gap: 0.0,
            new_line_size: font_size,
        });
    let baseline_y = (top + bottom + line_metrics.ascent + line_metrics.descent) / 2.0;

    let mut pen_x = x as f32;
    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, font_size);
        // 等宽数字：字形在统一宽度的格子里水平居中，步进取格子宽度
        let (shift, advance) = if tabular && ch.is_ascii_digit() {
            let cell = network_digit_advance();
            ((cell - metrics.advance_width) / 2.0, cell)
        } else {
            (0.0, metrics.advance_width)
        };
        // ymin 是位图底部相对 baseline 的偏移，位图顶部 = baseline - ymin - height
        let ox = (pen_x + shift) as i32 + metrics.xmin;
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
        pen_x += advance;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_rate_never_exceeds_template_shape() {
        let samples = [
            0.0,
            0.5,
            1.0,
            9.0,
            999.4,
            999.5,
            999.6,
            1_000.0,
            99_499.0,
            999_499.0,
            999_500.0,
            999_999.0,
            9_999_999.0,
            99_949_999.0,
            99_950_000.0,
            999_499_999.0,
            999_500_000.0,
            12_345_678_901.0,
            99_000_000_000.0,
        ];
        for rate in samples {
            let text = format_rate(rate);
            assert!(text.chars().count() <= 5, "{rate} -> {text}");
            assert!(!text.starts_with("1000"), "{rate} -> {text}");
        }
        assert_eq!(format_rate(999.6), "1K");
        assert_eq!(format_rate(999_600.0), "1.0M");
        assert_eq!(format_rate(99_960_000.0), "100M");
    }

    #[test]
    fn network_slot_covers_every_rate() {
        let slot = network_slot_width();
        for exp in 0..12 {
            for mantissa in [1.0, 1.1, 4.4, 7.7, 9.99] {
                let rate = mantissa * 10f64.powi(exp);
                let text = format_rate(rate);
                for arrow in ['↓', '↑'] {
                    let w = network_text_width(&format!("{arrow}{text}"));
                    assert!(w <= slot, "{rate} -> {text}: {w} > {slot}");
                }
            }
        }
    }

    #[test]
    fn icon_width_stable_when_rate_changes() {
        let config = StatusConfig {
            show_logo: false,
            show_cpu: false,
            show_memory: false,
            show_disk: false,
            show_network: true,
        };
        let mut widths = std::collections::BTreeSet::new();
        for (down, up) in [
            (0.0, 0.0),
            (1.0, 111.0),
            (888.0, 1_111.0),
            (11_111.0, 888_000.0),
            (1_111_111.0, 88_800_000.0),
            (111_000_000.0, 8_880_000_000.0),
        ] {
            let info = SystemInfo {
                hostname: String::new(),
                os_name: String::new(),
                os_version: String::new(),
                kernel_version: String::new(),
                uptime: 0,
                cpu_usage: 0.0,
                cpu_cores: 0,
                per_cpu_usage: Vec::new(),
                memory_total: 0,
                memory_used: 0,
                memory_usage: 0.0,
                swap_total: 0,
                swap_used: 0,
                disks: Vec::new(),
                networks: Vec::new(),
                net_down_rate: down,
                net_up_rate: up,
            };
            widths.insert(render_status_icon(&info, &config, true).width);
        }
        assert_eq!(widths.len(), 1, "宽度随速率变化：{widths:?}");
    }
}
