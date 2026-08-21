use fontdue::{Font, FontSettings, LineMetrics};
use std::sync::OnceLock;
use tauri::image::Image;

/// 图标高度（2x，Tauri 会缩放到菜单栏标准 18pt 高）
const ICON_HEIGHT: u32 = 36;
const FONT_SIZE: f32 = 24.0;
const PADDING_X: i32 = 7;
const DOT_DIAMETER: i32 = 12;
const GAP: i32 = 9;

fn font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        let bytes = std::fs::read("/System/Library/Fonts/SFNS.ttf")
            .or_else(|_| std::fs::read("/System/Library/Fonts/Monaco.ttf"))
            .expect("无法加载系统字体");
        Font::from_bytes(bytes, FontSettings::default()).expect("字体解析失败")
    })
}

/// 根据 CPU / 内存占用渲染状态栏图标（template 单色，黑色 + alpha）
pub fn render_status_icon(cpu: f32, mem: f32) -> Image<'static> {
    let cpu_text = format!("{cpu:.0}%");
    let mem_text = format!("{mem:.0}%");

    let w_cpu = text_width(&cpu_text) as i32;
    let w_mem = text_width(&mem_text) as i32;

    let width = (PADDING_X * 2 + DOT_DIAMETER + GAP * 2 + w_cpu + w_mem) as u32;
    let mut buf = vec![0u8; (width * ICON_HEIGHT * 4) as usize];

    draw_dot(&mut buf, width, PADDING_X, DOT_DIAMETER);

    let x_cpu = PADDING_X + DOT_DIAMETER + GAP;
    draw_text(&mut buf, width, x_cpu, &cpu_text);

    let x_mem = x_cpu + w_cpu + GAP;
    draw_text(&mut buf, width, x_mem, &mem_text);

    Image::new_owned(buf, width, ICON_HEIGHT)
}

fn text_width(text: &str) -> f32 {
    let mut width = 0.0;
    for ch in text.chars() {
        let (metrics, _) = font().rasterize(ch, FONT_SIZE);
        width += metrics.advance_width;
    }
    width
}

fn draw_dot(buf: &mut [u8], width: u32, x: i32, d: i32) {
    let cx = x + d / 2;
    let cy = ICON_HEIGHT as i32 / 2;
    let r = d / 2;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy > r * r {
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
        // 位图顶部坐标：ymin 是位图底部相对 baseline 的偏移
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
