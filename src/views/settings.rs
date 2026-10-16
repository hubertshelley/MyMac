//! 设置页（对应旧版 SettingsView.vue）：状态栏显示内容开关。

use eframe::egui::{self, RichText, Ui};
use mymac_core::config::{self, StatusConfig};
use shadcn_egui::components::icon::paint_icon;
use shadcn_egui::components::switch::Switch;
use shadcn_egui::icons::lucide;
use shadcn_egui::{Icon, Theme, layout};

use crate::state::Shared;

pub struct SettingsView {
    config: StatusConfig,
    loaded: bool,
    error: Option<String>,
}

impl SettingsView {
    pub fn new() -> Self {
        Self {
            config: StatusConfig::default(),
            loaded: false,
            error: None,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, shared: &Shared) {
        if !self.loaded {
            self.config = shared.config.lock().unwrap().clone();
            self.loaded = true;
        }

        let muted = Theme::current(ui.ctx()).palette.muted_foreground;
        ui.heading("设置");
        ui.add_space(4.0);
        ui.label(
            RichText::new("自定义菜单栏状态栏的显示内容。修改后立即生效，并会保存到本地。")
                .color(muted),
        );
        ui.add_space(12.0);

        if let Some(error) = &self.error {
            ui.colored_label(Theme::current(ui.ctx()).palette.destructive, error);
            ui.add_space(8.0);
        }

        type Entry = (
            &'static str,
            &'static str,
            Icon,
            fn(&mut StatusConfig) -> &mut bool,
        );
        let entries: [Entry; 5] = [
            (
                "Logo 图形",
                "在数据前显示彩色 Logo",
                lucide::GAUGE,
                |c| &mut c.show_logo,
            ),
            ("CPU 占用", "显示处理器使用率", lucide::CPU, |c| {
                &mut c.show_cpu
            }),
            (
                "内存占用",
                "显示内存使用率",
                lucide::MEMORY_STICK,
                |c| &mut c.show_memory,
            ),
            (
                "磁盘占用",
                "显示磁盘使用率",
                lucide::HARD_DRIVE,
                |c| &mut c.show_disk,
            ),
            (
                "网络速率",
                "显示实时下载速度",
                lucide::WIFI,
                |c| &mut c.show_network,
            ),
        ];

        layout::card(ui, |ui| {
            for (n, (label, desc, icon, get)) in entries.iter().enumerate() {
                if n > 0 {
                    ui.separator();
                }
                ui.horizontal(|ui| {
                    let secondary = Theme::current(ui.ctx()).palette.secondary;
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(6), secondary);
                    let icon_rect =
                        egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
                    paint_icon(
                        ui.painter(),
                        icon_rect,
                        icon,
                        muted,
                        Theme::current(ui.ctx()).metrics.icon_stroke,
                    );
                    ui.vertical(|ui| {
                        ui.label(RichText::new(*label).strong());
                        ui.label(RichText::new(*desc).small().color(muted));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut value = *get(&mut self.config);
                        if ui.add(Switch::new(&mut value)).changed() {
                            *get(&mut self.config) = value;
                            self.apply(shared);
                        }
                    });
                });
            }
        });
        ui.add_space(8.0);
        ui.label(
            RichText::new("提示：状态栏为彩色 Logo 加单色指标图标，图标与数字会跟随菜单栏深浅色主题自动切换。")
                .small()
                .color(muted),
        );
    }

    fn apply(&mut self, shared: &Shared) {
        if let Err(e) = config::save_config(&self.config) {
            self.error = Some(e);
            return;
        }
        *shared.config.lock().unwrap() = self.config.clone();
        self.error = None;
    }
}
