//! 资源监控页（对应旧版 DashboardView.vue）。

use eframe::egui::{self, RichText, Ui};
use mymac_core::system::SystemInfo;
use shadcn_egui::Theme;
use shadcn_egui::components::icon::IconView;
use shadcn_egui::icons::lucide;
use shadcn_egui::layout;

use crate::widgets::Progress;

pub fn show(ui: &mut Ui, info: Option<&SystemInfo>) {
    ui.heading("资源监控");
    ui.add_space(12.0);

    let Some(info) = info else {
        ui.label("正在采集系统信息…");
        return;
    };
    let muted = Theme::current(ui.ctx()).palette.muted_foreground;

    egui::ScrollArea::vertical().show(ui, |ui| {
        layout::vstack(ui, 12.0, |ui| {
            layout::card(ui, |ui| {
                card_title(ui, &lucide::CPU, "处理器", muted);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{:.1}%", info.cpu_usage)).size(28.0));
                    ui.label(RichText::new(format!("{} 核", info.cpu_cores)).color(muted));
                });
                ui.add(Progress::new(info.cpu_usage));
            });

            layout::card(ui, |ui| {
                card_title(ui, &lucide::MEMORY_STICK, "内存", muted);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{:.1}%", info.memory_usage)).size(28.0));
                    ui.label(
                        RichText::new(format!(
                            "{} / {}",
                            crate::fmt::bytes(info.memory_used),
                            crate::fmt::bytes(info.memory_total)
                        ))
                        .color(muted),
                    );
                });
                ui.add(Progress::new(info.memory_usage));
            });

            layout::card(ui, |ui| {
                card_title(ui, &lucide::HARD_DRIVE, "磁盘", muted);
                for disk in &info.disks {
                    ui.add_space(4.0);
                    layout::space_between(
                        ui,
                        |ui| {
                            ui.label(&disk.name);
                            ui.label(RichText::new(&disk.mount_point).color(muted));
                        },
                        |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} / {}",
                                    crate::fmt::bytes(disk.used),
                                    crate::fmt::bytes(disk.total)
                                ))
                                .color(muted),
                            );
                        },
                    );
                    ui.add(Progress::new(disk.usage));
                }
            });

            layout::card(ui, |ui| {
                card_title(ui, &lucide::WIFI, "网络", muted);
                ui.label(format!(
                    "↓ {}    ↑ {}",
                    mymac_core::status::format_rate(info.net_down_rate),
                    mymac_core::status::format_rate(info.net_up_rate)
                ));
            });
        });
    });
}

fn card_title(ui: &mut Ui, icon: &shadcn_egui::Icon, title: &str, muted: egui::Color32) {
    ui.horizontal(|ui| {
        ui.add(IconView::new(icon).size(16.0).color(muted));
        ui.label(RichText::new(title).strong());
    });
}
