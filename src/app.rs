//! 应用根：事件分发、托盘刷新与主窗口。

use std::sync::mpsc::Receiver;

use eframe::egui;
use shadcn_egui::Theme;
use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
use shadcn_egui::icons::lucide;

use crate::event::{UiEvent, UiSink};
use crate::macos;
use crate::nav::Page;
use crate::state::Shared;
use crate::theme::{self, Appearance};
use crate::tray::Tray;
use crate::views;
use crate::widgets::NavItem;

pub struct MyMacApp {
    receiver: Receiver<UiEvent>,
    #[allow(dead_code)] // M5 截图覆盖层会用来回传事件
    sink: UiSink,
    shared: Shared,
    /// 托盘菜单句柄只能在主线程使用，由 `logic`（主线程）持有并更新
    tray: Tray,
    page: Page,
    appearance: Appearance,
    launch: views::launch::LaunchView,
}

impl MyMacApp {
    pub fn new(receiver: Receiver<UiEvent>, sink: UiSink, shared: Shared, tray: Tray) -> Self {
        Self {
            receiver,
            sink,
            shared,
            tray,
            page: Page::default(),
            appearance: Appearance::default(),
            launch: views::launch::LaunchView::new(),
        }
    }

    fn show_main_window(ctx: &egui::Context) {
        macos::set_dock_visible(true);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

impl eframe::App for MyMacApp {
    /// 窗口隐藏时唯一仍会执行的入口：消费托盘 / 快捷键 / 采样事件并更新托盘。
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                UiEvent::ShowMainWindow => Self::show_main_window(ctx),
                UiEvent::Navigate(page) => {
                    self.page = page;
                    Self::show_main_window(ctx);
                }
                UiEvent::StartScreenshot => {
                    // M5：抓屏并显示/替换覆盖层。当前仅记录。
                    log::info!("收到截图触发（M5 实现）");
                }
                UiEvent::SystemSampled => {
                    let info = self.shared.latest.lock().unwrap().clone();
                    if let Some(info) = info {
                        self.tray.refresh(&info, &self.shared);
                    }
                }
                UiEvent::ClipboardChanged => self.tray.rebuild_clip_menu(&self.shared),
                UiEvent::TotpCopied(result) => {
                    if let Err(error) = result {
                        log::error!("复制 2FA 验证码失败：{error}");
                    }
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

        let palette = Theme::current(&ctx).palette;

        egui::Panel::left("sidebar")
            .exact_size(200.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(palette.card)
                    .stroke(egui::Stroke::new(1.0_f32, palette.border))
                    .inner_margin(egui::Margin::same(12)),
            )
            .show_inside(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette.background)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show_inside(ui, |ui| match self.page {
                Page::Dashboard => {
                    let info = self.shared.latest.lock().unwrap().clone();
                    views::dashboard::show(ui, info.as_ref());
                }
                Page::Launch => self.launch.show(ui),
                page => views::placeholder::show(ui, page),
            });
    }
}

impl MyMacApp {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.heading("MyMac");
        ui.add_space(12.0);

        for page in Page::ALL {
            let icon = page.icon();
            let item = NavItem::new(page.label(), &icon, self.page == page);
            if ui.add(item).clicked() {
                self.page = page;
            }
            ui.add_space(2.0);
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            let (icon, tip) = match self.appearance {
                Appearance::Dark => (lucide::SUN, "切换到浅色"),
                Appearance::Light => (lucide::MOON, "切换到深色"),
            };
            let button = Button::new(tip)
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Icon)
                .icon(&icon);
            if ui.add(button).on_hover_text(tip).clicked() {
                self.appearance = self.appearance.toggled();
                theme::switch(ui.ctx(), self.appearance);
            }
        });
    }
}
