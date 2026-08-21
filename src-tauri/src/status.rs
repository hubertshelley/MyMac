use fontdue::{Font, FontSettings, LineMetrics};
use std::sync::OnceLock;
use tauri::image::Image;

use crate::config::StatusConfig;
use crate::system::SystemInfo;

/// 图标高度（2x，Tauri 会缩放到菜单栏标准 18pt 高）
const ICON_HEIGHT: u32 = 36;
const FONT_SIZE: f32 = 24.0;
const PADDING_X: i32 = 7;
const LOGO_SIZE: i32 = 18;
const GAP: i32 = 9;

enum Part {
    Logo,
    Text(String),
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

/// 根据配置与系统快照渲染状态栏图标（template 单色，黑色 + alpha）
pub fn render_status_icon(info: &SystemInfo, config: &StatusConfig) -> Image<'static> {
    let mut parts: Vec<Part> = Vec::new();
    if config.show_logo {
        parts.push(Part::Logo);
    }
    if config.show_cpu {
        parts.push(Part::Text(format!("{:.0}%", info.cpu_usage)));
    }
    if config.show_memory {
        parts.push(Part::Text(format!("{:.0}%", info.memory_usage)));
    }
    if config.show_disk {
        if let Some(d) = info.disks.first() {
            parts.push(Part::Text(format!("{:.0}%", d.usage)));
        }
    }
    if parts.is_empty() {
        parts.push(Part::Text("--".to_string()));
    }

    // 计算总宽度
    let mut width = PADDING_X * 2;
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            width += GAP;
        }
        width += match part {
            Part::Logo => LOGO_SIZE,
            Part::Text(t) => text_width(t) as i32,
        };
    }

    let mut buf = vec![0u8; (width as u32 * ICON_HEIGHT * 4) as usize];

    let mut x = PADDING_X;
    for part in &parts {
        match part {
            Part::Logo => {
                draw_ring(&mut buf, width as u32, x, LOGO_SIZE);
                x += LOGO_SIZE;
            }
            Part::Text(t) => {
                draw_text(&mut buf, width as u32, x, t);
                x += text_width(t) as i32;
            }
        }
        x += GAP;
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

/// 渲染一个圆环（Logo 图形），内部留白
fn draw_ring(buf: &mut [u8], width: u32, x: i32, d: i32) {
    let cx = x + d / 2;
    let cy = ICON_HEIGHT as i32 / 2;
    let outer = d / 2;
    let inner = d * 3 / 10; // 内径，形成圆环
    let outer2 = outer * outer;
    let inner2 = inner * inner;
    for dy in -outer..=outer {
        for dx in -outer..=outer {
            let dist2 = dx * dx + dy * dy;
            if dist2 > outer2 || dist2 < inner2 {
                continue;
            }
            let px = cx + dx;
            let py = cy + dy;
            if px < 0 || py < 0 || px >= width as i32 || py >= ICON_HEIGHT as i32 {
                continue;
            }
            let idx = ((py as u32 * width + px as u32) * 4) as usize;
            buf[idx + 3] = 255;
        }
    }
}

fn draw_text(buf: &mut [u8], width: u32, x: i32, text: &str) {
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
                buf[idx + 3] = cov; // 黑色 + alpha，作为 template 图标
            }
        }
        pen_x += metrics.advance_width;
    }
}
