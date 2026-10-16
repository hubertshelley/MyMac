//! 主题与字体：shadcn/ui 设计体系（shadcn-egui）+ 系统中文字体回退。
//!
//! 顺序很重要：先把 PingFang 放进 Proportional 回退链，再用 `apply_with_fonts`
//! 应用主题——库会把主字体（Inter）放在回退链最前，中文字形缺失时落到 PingFang。

use eframe::egui;
use shadcn_egui::Theme;

/// 当前外观：明暗 + 主题色板。后续可在设置页持久化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
}

impl Appearance {
    pub fn theme(self) -> Theme {
        match self {
            Appearance::Dark => Theme::vega_dark(),
            Appearance::Light => Theme::vega_light(),
        }
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

fn with_cjk_fonts(mut fonts: egui::FontDefinitions) -> egui::FontDefinitions {
    const CANDIDATES: [&str; 3] = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
    ];
    for path in CANDIDATES {
        match std::fs::read(path) {
            Ok(data) => {
                fonts
                    .font_data
                    .insert("cjk".into(), egui::FontData::from_owned(data).into());
                if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                    list.push("cjk".into());
                }
                if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                    list.push("cjk".into());
                }
                log::info!("中文字体已加载：{path}");
                return fonts;
            }
            Err(error) => log::warn!("中文字体 {path} 加载失败：{error}"),
        }
    }
    log::error!("未找到可用的中文字体，中文将显示为方块");
    fonts
}
