//! 应用根：事件分发与主窗口。

use std::sync::mpsc::Receiver;

use eframe::egui;
use shadcn_egui::components::badge::Badge;
use shadcn_egui::components::button::Button;
use shadcn_egui::layout;

use crate::event::{UiEvent, UiSink};
use crate::macos;
use crate::theme::{self, Appearance};

pub struct MyMacApp {
    receiver: Receiver<UiEvent>,
    #[allow(dead_code)] // M5 截图覆盖层会用来回传事件
    sink: UiSink,
    appearance: Appearance,
}

impl MyMacApp {
    pub fn new(receiver: Receiver<UiEvent>, sink: UiSink) -> Self {
        Self {
            receiver,
            sink,
            appearance: Appearance::default(),
        }
    }

    fn show_main_window(ctx: &egui::Context) {
        macos::set_dock_visible(true);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

impl eframe::App for MyMacApp {
    /// 窗口隐藏时唯一仍会执行的入口：消费托盘 / 快捷键事件。
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                UiEvent::ShowMainWindow => Self::show_main_window(ctx),
                UiEvent::StartScreenshot => {
                    // M5：抓屏并显示/替换覆盖层。当前仅记录。
                    log::info!("收到截图触发（M5 实现）");
                }
                UiEvent::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // 关闭主窗口 = 隐藏窗口与 Dock 图标，托盘常驻
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            macos::set_dock_visible(false);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        let palette = shadcn_egui::Theme::current(&ctx).palette;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette.background)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show_inside(ui, |ui| {
                layout::row(ui, 12.0, |ui| {
                    ui.heading("MyMac 电脑管家");
                    ui.add(Badge::new("egui + shadcn"));
                });
                ui.add_space(16.0);
                ui.label("托盘、F1、关闭隐藏 Dock 已就绪；页面将在 M2/M3 迁入。");
                ui.add_space(16.0);
                layout::row(ui, 8.0, |ui| {
                    let label = match self.appearance {
                        Appearance::Dark => "切换到浅色",
                        Appearance::Light => "切换到深色",
                    };
                    if ui.add(Button::new(label)).clicked() {
                        self.appearance = self.appearance.toggled();
                        theme::switch(&ctx, self.appearance);
                    }
                });
            });
    }
}
