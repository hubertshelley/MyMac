//! 2FA 验证码页（对应旧版 TotpView.vue）。

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use mymac_core::clipboard::ClipboardState;
use mymac_core::totp::{self, TotpAccountView, TotpState};
use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
use shadcn_egui::components::dialog::{self, Dialog};
use shadcn_egui::components::input::Input;
use shadcn_egui::components::select::Select;
use shadcn_egui::icons::lucide;
use shadcn_egui::{Theme, layout};

use crate::task::{Poll, Task};
use crate::widgets::Progress;

/// 后台动作（钥匙串读写、二维码识别都放线程里）
enum Job {
    Add,
    Copy,
    Delete,
    /// 二维码识别成功，回填 secret
    Qr,
}

pub struct TotpView {
    totp_state: Arc<TotpState>,
    clip_state: Arc<ClipboardState>,
    accounts: Vec<TotpAccountView>,
    accounts_at: Option<Instant>,
    name: String,
    issuer: String,
    secret: String,
    digits: usize,
    period: usize,
    error: Option<String>,
    toast: Option<(String, Instant)>,
    confirm_delete: Option<(String, String)>,
    task: Option<(Job, Task<Result<String, String>>)>,
}

const DIGITS: [&str; 2] = ["6 位", "8 位"];
const PERIODS: [&str; 2] = ["30 秒", "60 秒"];

impl TotpView {
    pub fn new(totp_state: Arc<TotpState>, clip_state: Arc<ClipboardState>) -> Self {
        Self {
            totp_state,
            clip_state,
            accounts: Vec::new(),
            accounts_at: None,
            name: String::new(),
            issuer: String::new(),
            secret: String::new(),
            digits: 0,
            period: 0,
            error: None,
            toast: None,
            confirm_delete: None,
            task: None,
        }
    }

    fn toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }

    fn refresh(&mut self, state: &TotpState) {
        self.accounts = totp::get_totp_accounts(state).unwrap_or_default();
        self.accounts_at = Some(Instant::now());
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // 每秒刷新验证码（秒级变化，无需更频繁）
        if self.accounts_at.is_none()
            || self.accounts_at.unwrap().elapsed() >= Duration::from_secs(1)
        {
            let state = self.totp_state.clone();
            self.refresh(&state);
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        // 后台动作结果
        if let Some((job, task)) = &self.task {
            match task.poll() {
                Poll::Ready(Ok(value)) => {
                    match job {
                        Job::Add => {
                            self.toast("账户已添加");
                            self.name.clear();
                            self.issuer.clear();
                            self.secret.clear();
                            self.digits = 0;
                            self.period = 0;
                            let state = self.totp_state.clone();
                            self.refresh(&state);
                        }
                        Job::Copy => self.toast(format!("已复制 {value}")),
                        Job::Delete => {
                            self.toast("账户已删除");
                            let state = self.totp_state.clone();
                            self.refresh(&state);
                        }
                        Job::Qr => {
                            self.secret = value;
                            self.toast("二维码识别成功，已自动填入");
                        }
                    }
                    self.task = None;
                }
                Poll::Ready(Err(e)) => {
                    self.error = Some(e);
                    self.task = None;
                }
                Poll::Lost => {
                    self.error = Some("后台任务异常结束".into());
                    self.task = None;
                }
                Poll::Pending => {}
            }
        }

        let muted = Theme::current(&ctx).palette.muted_foreground;
        ui.heading("2FA 验证码");
        ui.add_space(12.0);
        self.account_list(ui, &ctx, muted);
        ui.add_space(16.0);
        self.add_form(ui, &ctx, muted);
        self.delete_dialog(&ctx);
        self.draw_toast(&ctx);
    }

    fn account_list(&mut self, ui: &mut Ui, ctx: &egui::Context, muted: egui::Color32) {
        let mut copy: Option<String> = None;
        let mut delete: Option<(String, String)> = None;

        egui::ScrollArea::vertical()
            .id_salt("totp_list")
            .max_height(280.0)
            .show(ui, |ui| {
                layout::card(ui, |ui| {
                    if self.accounts.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(12.0);
                            ui.label(RichText::new("暂无账户，请在下方添加").color(muted));
                            ui.add_space(12.0);
                        });
                    }
                    for (n, account) in self.accounts.iter().enumerate() {
                        if n > 0 {
                            ui.separator();
                        }
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_max_width((ui.available_width() - 140.0).max(160.0));
                                ui.label(RichText::new(&account.name).strong());
                                if !account.issuer.is_empty() {
                                    ui.label(RichText::new(&account.issuer).small().color(muted));
                                }
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let del = Button::new("删除")
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::IconSm)
                                        .icon(&lucide::TRASH_2);
                                    if ui.add(del).on_hover_text("删除").clicked() {
                                        delete = Some((account.id.clone(), account.name.clone()));
                                    }
                                    if ui
                                        .add(
                                            Button::new("复制")
                                                .size(ButtonSize::Sm)
                                                .icon(&lucide::COPY),
                                        )
                                        .clicked()
                                    {
                                        copy = Some(account.id.clone());
                                    }
                                    ui.label(
                                        RichText::new(format!(
                                            "{} {}",
                                            display_code(&account.code),
                                            "●".repeat(account.remaining.min(10) as usize)
                                        ))
                                        .monospace()
                                        .size(20.0),
                                    );
                                },
                            );
                        });
                        // 剩余时间进度（从满到空）
                        let elapsed = (account.period - account.remaining) as f32;
                        ui.add(Progress::new(elapsed / account.period as f32 * 100.0));
                    }
                });
            });

        if let Some(id) = copy {
            let totp_state = self.totp_state.clone();
            let clip_state = self.clip_state.clone();
            self.task = Some((
                Job::Copy,
                Task::spawn(ctx, move || {
                    totp::copy_totp_code(&totp_state, &clip_state, id)
                }),
            ));
        }
        if let Some((id, _)) = delete {
            self.confirm_delete = Some(delete_details(&self.accounts, &id));
        }
    }

    fn add_form(&mut self, ui: &mut Ui, ctx: &egui::Context, muted: egui::Color32) {
        let busy = matches!(self.task.as_ref().map(|(j, _)| j), Some(Job::Add))
            || matches!(self.task.as_ref().map(|(j, _)| j), Some(Job::Qr));

        layout::card(ui, |ui| {
            ui.label(RichText::new("添加账户").strong());
            ui.add_space(8.0);
            if let Some(error) = &self.error {
                ui.colored_label(Theme::current(ctx).palette.destructive, error);
                ui.add_space(4.0);
            }
            egui::Grid::new("totp_form")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("账户名称");
                    ui.add_sized([320.0, 32.0], Input::new(&mut self.name).hint("如：GitHub"));
                    ui.end_row();
                    ui.label("服务商");
                    ui.add_sized([320.0, 32.0], Input::new(&mut self.issuer).hint("可选"));
                    ui.end_row();
                    ui.label("密钥 / otpauth 链接");
                    ui.add_sized(
                        [320.0, 32.0],
                        Input::new(&mut self.secret)
                            .hint("Base32 密钥，或直接粘贴 otpauth://totp/… 链接"),
                    );
                    ui.end_row();
                    ui.label("位数");
                    ui.add(Select::new("totp_digits", &mut self.digits, &DIGITS));
                    ui.end_row();
                    ui.label("周期");
                    ui.add(Select::new("totp_period", &mut self.period, &PERIODS));
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "粘贴 otpauth 链接时，名称、服务商、位数、周期会自动解析，无需手填。",
                )
                .small()
                .color(muted),
            );
            ui.add_space(8.0);
            layout::row(ui, 8.0, |ui| {
                let add = Button::new(if busy { "添加中…" } else { "添加账户" })
                    .icon(&lucide::PLUS)
                    .enabled(!busy);
                if ui.add(add).clicked() {
                    let name = self.name.clone();
                    let issuer = self.issuer.clone();
                    let secret = self.secret.clone();
                    let digits = [6, 8][self.digits];
                    let period = [30, 60][self.period];
                    let state = self.totp_state.clone();
                    self.error = None;
                    self.task = Some((
                        Job::Add,
                        Task::spawn(ctx, move || {
                            totp::add_totp_account(&state, name, issuer, secret, digits, period)
                                .map(|v| v.name)
                        }),
                    ));
                }
                let capture = Button::new("屏幕截图识别")
                    .variant(ButtonVariant::Outline)
                    .icon(&lucide::CAMERA)
                    .enabled(!busy);
                if ui.add(capture).clicked() {
                    self.error = None;
                    self.task = Some((Job::Qr, Task::spawn(ctx, totp::capture_totp_qr)));
                }
                let paste = Button::new("从剪贴板识别")
                    .variant(ButtonVariant::Outline)
                    .icon(&lucide::CLIPBOARD_PASTE)
                    .enabled(!busy);
                if ui.add(paste).clicked() {
                    self.error = None;
                    self.task = Some((Job::Qr, Task::spawn(ctx, totp::decode_totp_qr_clipboard)));
                }
            });
        });
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some((id, name)) = self.confirm_delete.clone() else {
            return;
        };
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        let description = format!("删除账户「{name}」？密钥会从钥匙串同步移除。");
        Dialog::new("totp_delete", "删除账户")
            .description(&description)
            .show(ctx, &mut open, |ui| {
                dialog::footer(ui, |ui| {
                    confirm |= ui
                        .add(Button::new("删除").variant(ButtonVariant::Destructive))
                        .clicked();
                    cancel |= ui
                        .add(Button::new("取消").variant(ButtonVariant::Outline))
                        .clicked();
                });
            });
        if confirm {
            self.confirm_delete = None;
            self.error = None;
            let state = self.totp_state.clone();
            self.task = Some((
                Job::Delete,
                Task::spawn(ctx, move || {
                    totp::delete_totp_account(&state, id).map(|_| String::new())
                }),
            ));
        } else if cancel || !open {
            self.confirm_delete = None;
        }
    }

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some((message, at)) = &self.toast else {
            return;
        };
        if at.elapsed().as_secs() >= 2 {
            self.toast = None;
            return;
        }
        egui::Area::new(egui::Id::new("totp-toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
            .interactable(false)
            .show(ctx, |ui| {
                let palette = Theme::current(ctx).palette;
                egui::Frame::new()
                    .fill(palette.foreground)
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .show(ui, |ui| {
                        ui.label(RichText::new(message).color(palette.background).small());
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

fn display_code(code: &str) -> String {
    let middle = code.len().div_ceil(2);
    format!("{} {}", &code[..middle], &code[middle..])
}

fn delete_details(accounts: &[TotpAccountView], id: &str) -> (String, String) {
    accounts
        .iter()
        .find(|a| a.id == id)
        .map(|a| (a.id.clone(), a.name.clone()))
        .unwrap_or_else(|| (id.to_string(), String::new()))
}
