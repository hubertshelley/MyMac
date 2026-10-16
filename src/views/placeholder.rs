//! 尚未迁移的页面占位（M3 起逐页替换）。

use eframe::egui::Ui;

use crate::nav::Page;

pub fn show(ui: &mut Ui, page: Page) {
    ui.heading(page.label());
    ui.add_space(8.0);
    ui.label("该页面将在 M3 迁入。");
}
