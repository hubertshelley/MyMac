//! 主题与字体：shadcn/ui 设计体系（shadcn-egui）+ 系统中文字体回退。
//!
//! 顺序很重要：先把 PingFang 放进 Proportional 回退链，再用 `apply_with_fonts`
//! 应用主题——库会把主字体（Inter）放在回退链最前，中文字形缺失时落到 PingFang。

use eframe::egui;
use shadcn_egui::style::Style;
use shadcn_egui::{Palette, Theme};

/// 样式与圆角：nova 比默认的 vega 更紧凑，圆角取 8 接近旧版 Tailwind 的 rounded-lg。
const RADIUS: f32 = 8.0;

/// 当前外观：明暗 + 主题色板。后续可在设置页持久化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
}

impl Appearance {
    pub fn theme(self) -> Theme {
        let palette = match self {
            Appearance::Dark => Palette::dark(),
            Appearance::Light => Palette::light(),
        };
        Theme::new(Style::nova(), palette, RADIUS)
    }

    pub fn toggled(self) -> Self {
        match self {
            Appearance::Dark => Appearance::Light,
            Appearance::Light => Appearance::Dark,
        }
    }
}

/// 首次启动：装中文字体并应用主题。
pub fn install(ctx: &egui::Context, appearance: Appearance) {
    let base = with_cjk_fonts(egui::FontDefinitions::default());
    appearance.theme().apply_with_fonts(ctx, base);
}

/// 切换明暗：沿用 ctx 当前字体定义，避免重复叠加。
pub fn switch(ctx: &egui::Context, appearance: Appearance) {
    let base = ctx.fonts(|f| f.definitions().clone());
    appearance.theme().apply_with_fonts(ctx, base);
}

/// 中文字体的垂直视觉补偿。正值下移、负值上移，按字号百分比生效。
///
/// Inter 自带 -0.07 的补偿让拉丁字形在控件里居中；中文走 Hiragino 回退，
/// 其度量和 Inter 不同（Hiragino 字形实测偏高，2026-10 在 macOS 27 实机校准为 0.2）。
/// 环境变量 `MYMAC_CJK_Y` 可临时覆盖，便于换字体或系统后重新校准。
fn cjk_y_offset_factor() -> f32 {
    static VALUE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("MYMAC_CJK_Y")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.2)
    })
}

fn with_cjk_fonts(mut fonts: egui::FontDefinitions) -> egui::FontDefinitions {
    const CANDIDATES: [&str; 3] = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
    ];
    for path in CANDIDATES {
        match std::fs::read(path) {
            Ok(data) => {
                // shadcn-egui 给 Inter 设了 y_offset_factor = -0.07 让文字在按钮里视觉居中；
                // 中文字形来自这个回退字体，需要同样的垂直补偿，否则中文会比英文低一截。
                let tweak = egui::FontTweak {
                    y_offset_factor: cjk_y_offset_factor(),
                    ..Default::default()
                };
                fonts.font_data.insert(
                    "cjk".into(),
                    egui::FontData::from_owned(data).tweak(tweak).into(),
                );
                if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                    list.push("cjk".into());
                }
                if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                    list.push("cjk".into());
                }
                log::info!("中文字体已加载：{path}");
                return fonts;
            }
            Err(error) => log::warn!("中文字体 {path} 不可用：{error}"),
        }
    }
    log::error!("未找到可用的中文字体，中文将显示为方块");
    fonts
}
