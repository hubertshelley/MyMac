//! 应用根：事件分发、托盘刷新与主窗口。

use std::sync::mpsc::Receiver;

use eframe::egui;
use shadcn_egui::Theme;
use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
use shadcn_egui::components::icon::paint_icon;
use shadcn_egui::icons::lucide;

use crate::event::{UiEvent, UiSink};
use crate::macos;
use crate::nav::Page;
use crate::screenshot;
use crate::state::Shared;
use crate::theme::{self, Appearance};
use crate::tray::Tray;
use crate::views;
use crate::widgets::NavItem;

pub struct MyMacApp {
    receiver: Receiver<UiEvent>,
    sink: UiSink,
    shared: Shared,
    /// 托盘菜单句柄只能在主线程使用，由 `logic`（主线程）持有并更新
    tray: Tray,
    page: Page,
    appearance: Appearance,
    launch: views::launch::LaunchView,
    apps: views::apps::AppsView,
    clipboard: views::clipboard::ClipboardView,
    settings: views::settings::SettingsView,
    totp: views::totp::TotpView,
    brew: views::brew::BrewView,
    screenshot: screenshot::ScreenshotManager,
    /// 正在退出：此时窗口的关闭请求要放行，不能再当成「关闭 = 隐藏」拦截
    quitting: bool,
}

impl MyMacApp {
    pub fn new(receiver: Receiver<UiEvent>, sink: UiSink, shared: Shared, tray: Tray) -> Self {
        let totp = views::totp::TotpView::new(shared.totp.clone(), shared.clipboard.clone());
        Self {
            receiver,
            sink,
            shared,
            tray,
            page: Page::default(),
            appearance: Appearance::default(),
            launch: views::launch::LaunchView::new(),
            apps: views::apps::AppsView::new(),
            clipboard: views::clipboard::ClipboardView::new(),
            settings: views::settings::SettingsView::new(),
            totp,
            brew: views::brew::BrewView::new(),
            screenshot: screenshot::ScreenshotManager::new(),
            quitting: false,
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
                UiEvent::StartScreenshot => self.screenshot.start(ctx),
                UiEvent::SystemSampled => {
                    let info = self.shared.latest.lock().unwrap().clone();
                    if let Some(info) = info {
                        self.tray.refresh(&info, &self.shared);
                    }
                }
                UiEvent::AppSize(info) => self.apps.on_app_size(info),
                UiEvent::ClipboardChanged => {
                    self.tray.rebuild_clip_menu(&self.shared);
                    self.clipboard.mark_stale();
                }
                UiEvent::TotpCopied(result) => {
                    if let Err(error) = result {
                        log::error!("复制 2FA 验证码失败：{error}");
                    }
                }
                UiEvent::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    // Close 只是往视口事件队列里排一个 Close，要等下一轮 pass 才会被
                    // eframe 读到并真正关闭；不主动请求重绘的话，要等下一次托盘/定时唤醒。
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // 关闭主窗口 = 隐藏窗口与 Dock 图标，托盘常驻
        if !self.quitting && ctx.input(|i| i.viewport().close_requested()) {
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

        // 截图覆盖层视口（隐藏态注册；显隐由 ViewportCommand 驱动）
        self.screenshot.register(ui);

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
                Page::Apps => self.apps.show(ui, &self.sink),
                Page::Clipboard => self.clipboard.show(ui, &self.shared.clipboard),
                Page::Settings => self.settings.show(ui, &self.shared),
                Page::Totp => self.totp.show(ui),
                Page::Brew => self.brew.show(ui),
                Page::Launch => self.launch.show(ui),
            });
    }
}

impl MyMacApp {
    /// 顶部品牌区：主色圆角 Logo 块 + 「MyMac 管家」（与旧版一致）。
    fn brand(&self, ui: &mut egui::Ui) {
        let palette = Theme::current(ui.ctx()).palette;
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(6), palette.primary);
            let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
            paint_icon(
                ui.painter(),
                icon_rect,
                &lucide::GAUGE,
                palette.primary_foreground,
                Theme::current(ui.ctx()).metrics.icon_stroke,
            );
            ui.label(egui::RichText::new("MyMac 管家").strong());
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        self.brand(ui);
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
