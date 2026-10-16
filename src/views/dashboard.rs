//! 资源监控页（对应旧版 DashboardView.vue：两列卡片 + 全宽磁盘 + 网络/系统信息两列）。

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
    let palette = Theme::current(ui.ctx()).palette;
    let muted = palette.muted_foreground;

    egui::ScrollArea::vertical()
        .id_salt("dashboard_scroll")
        .auto_shrink(false)
        .show(ui, |ui| {
            // 处理器 | 内存
            two_columns(
                ui,
                "dash_top_cols",
                |ui| {
                    layout::card(ui, |ui| {
                        card_title(ui, &lucide::CPU, "处理器", muted);
                        ui.add_space(6.0);
                        layout::space_between(
                            ui,
                            |ui| {
                                ui.label(
                                    RichText::new(format!("{:.1}%", info.cpu_usage)).size(30.0),
                                );
                            },
                            |ui| {
                                ui.label(
                                    RichText::new(format!("{} 核", info.cpu_cores))
                                        .small()
                                        .color(muted),
                                );
                            },
                        );
                        ui.add(Progress::new(info.cpu_usage));
                        if !info.per_cpu_usage.is_empty() {
                            ui.add_space(10.0);
                            per_core_grid(ui, &info.per_cpu_usage, palette.secondary, muted);
                        }
                    });
                },
                |ui| {
                    layout::card(ui, |ui| {
                        card_title(ui, &lucide::MEMORY_STICK, "内存", muted);
                        ui.add_space(6.0);
                        layout::space_between(
                            ui,
                            |ui| {
                                ui.label(
                                    RichText::new(format!("{:.1}%", info.memory_usage)).size(30.0),
                                );
                            },
                            |ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{} / {}",
                                        crate::fmt::bytes(info.memory_used),
                                        crate::fmt::bytes(info.memory_total)
                                    ))
                                    .small()
                                    .color(muted),
                                );
                            },
                        );
                        ui.add(Progress::new(info.memory_usage));
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!(
                                "交换分区：{} / {}",
                                crate::fmt::bytes(info.swap_used),
                                crate::fmt::bytes(info.swap_total)
                            ))
                            .small()
                            .color(muted),
                        );
                    });
                },
            );
            ui.add_space(12.0);

            // 磁盘（全宽）
            layout::card(ui, |ui| {
                card_title(ui, &lucide::HARD_DRIVE, "磁盘", muted);
                for disk in &info.disks {
                    ui.add_space(8.0);
                    layout::space_between(
                        ui,
                        |ui| {
                            ui.label(RichText::new(&disk.name).strong());
                            ui.label(RichText::new(&disk.mount_point).small().color(muted));
                        },
                        |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{:.1}% · {} / {}",
                                    disk.usage,
                                    crate::fmt::bytes(disk.used),
                                    crate::fmt::bytes(disk.total)
                                ))
                                .small()
                                .color(muted),
                            );
                        },
                    );
                    ui.add(Progress::new(disk.usage));
                }
            });
            ui.add_space(12.0);

            // 网络 | 系统信息
            two_columns(
                ui,
                "dash_bottom_cols",
                |ui| {
                    layout::card(ui, |ui| {
                        card_title(ui, &lucide::WIFI, "网络", muted);
                        ui.add_space(6.0);
                        for n in &info.networks {
                            layout::space_between(
                                ui,
                                |ui| {
                                    ui.label(RichText::new(&n.name).strong().size(13.0));
                                },
                                |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "↓ {} · ↑ {}",
                                            crate::fmt::bytes(n.received),
                                            crate::fmt::bytes(n.transmitted)
                                        ))
                                        .small()
                                        .color(muted),
                                    );
                                },
                            );
                        }
                    });
                },
                |ui| {
                    layout::card(ui, |ui| {
                        card_title(ui, &lucide::SERVER, "系统信息", muted);
                        ui.add_space(6.0);
                        info_row(ui, "主机名", &info.hostname, muted);
                        info_row(
                            ui,
                            "系统",
                            &format!("{} {}", info.os_name, info.os_version),
                            muted,
                        );
                        info_row(ui, "内核", &info.kernel_version, muted);
                        layout::space_between(
                            ui,
                            |ui| {
                                ui.horizontal(|ui| {
                                    ui.add(IconView::new(&lucide::TIMER).size(13.0).color(muted));
                                    ui.label(RichText::new("运行时间").color(muted));
                                });
                            },
                            |ui| {
                                ui.label(format_uptime(info.uptime));
                            },
                        );
                    });
                },
            );
        });
}

/// 等宽两列。用 Grid 而不是 `ui.columns`，保证列间距与卡片间隔一致。
fn two_columns<R>(
    ui: &mut Ui,
    id: &str,
    left: impl FnOnce(&mut Ui) -> R,
    right: impl FnOnce(&mut Ui) -> R,
) {
    let gap = 12.0;
    let col = ((ui.available_width() - gap) / 2.0).max(120.0);
    egui::Grid::new(egui::Id::new(id))
        .num_columns(2)
        .spacing([gap, gap])
        .min_col_width(col)
        .show(ui, |ui| {
            left(ui);
            right(ui);
            ui.end_row();
        });
}

/// 每核占用：4 列小方块（百分比 + 核编号），对应旧版 grid-cols-4。
fn per_core_grid(ui: &mut Ui, usage: &[f32], fill: egui::Color32, muted: egui::Color32) {
    egui::Grid::new("per_core")
        .num_columns(4)
        .spacing([8.0, 8.0])
        .min_col_width((ui.available_width() - 24.0) / 4.0)
        .show(ui, |ui| {
            for (i, u) in usage.iter().enumerate() {
                if i > 0 && i % 4 == 0 {
                    ui.end_row();
                }
                egui::Frame::new()
                    .fill(fill)
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(0, 8))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(format!("{:.0}%", u)).small().strong());
                            ui.label(
                                RichText::new(format!("核 {}", i + 1))
                                    .size(10.0)
                                    .color(muted),
                            );
                        });
                    });
            }
            ui.end_row();
        });
}

fn info_row(ui: &mut Ui, label: &str, value: &str, muted: egui::Color32) {
    layout::space_between(
        ui,
        |ui| {
            ui.label(RichText::new(label).color(muted));
        },
        |ui| {
            ui.label(RichText::new(value).size(13.0));
        },
    );
}

fn card_title(ui: &mut Ui, icon: &shadcn_egui::Icon, title: &str, muted: egui::Color32) {
    ui.horizontal(|ui| {
        ui.add(IconView::new(icon).size(16.0).color(muted));
        ui.label(RichText::new(title).strong());
    });
}

/// 秒数 →「N 天 N 小时」/「N 小时 N 分钟」/「N 分钟」。
fn format_uptime(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days} 天 {hours} 小时")
    } else if hours > 0 {
        format!("{hours} 小时 {minutes} 分钟")
    } else {
        format!("{minutes} 分钟")
    }
}
