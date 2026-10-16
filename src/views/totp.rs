//! 2FA 验证码页（对应旧版 TotpView.vue；「添加」改为弹窗，新增图片导入）。

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use mymac_core::clipboard::ClipboardState;
use mymac_core::totp::{self, TotpAccountView, TotpState};
use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
use shadcn_egui::components::dialog::{self, Dialog};
use shadcn_egui::components::icon::paint_icon;
use shadcn_egui::components::input::Input;
use shadcn_egui::components::select::Select;
use shadcn_egui::icons::lucide;
use shadcn_egui::{Theme, layout};

use crate::task::{Poll, Task};
use crate::widgets::Progress;

enum Job {
    Add,
    Copy,
    Delete,
    /// 二维码识别成功，回填 secret
    Qr,
}

const DIGITS: [&str; 2] = ["6 位", "8 位"];
const PERIODS: [&str; 2] = ["30 秒", "60 秒"];

pub struct TotpView {
    totp_state: Arc<TotpState>,
    clip_state: Arc<ClipboardState>,
    accounts: Vec<TotpAccountView>,
    /// 数据加载层的常驻错误（钥匙串未授权等）
    load_error: Option<String>,
    accounts_at: Option<Instant>,
    /// 添加账户弹窗是否打开
    add_open: bool,
    name: String,
    issuer: String,
    secret: String,
    digits: usize,
    period: usize,
    error: Option<String>,
    toast: Option<(String, Instant)>,
    /// 卡片内行内确认删除的账户 id
    pending_delete: Option<String>,
    task: Option<(Job, Task<Result<String, String>>)>,
}

impl TotpView {
    pub fn new(totp_state: Arc<TotpState>, clip_state: Arc<ClipboardState>) -> Self {
        Self {
            totp_state,
            clip_state,
            accounts: Vec::new(),
            load_error: None,
            accounts_at: None,
            add_open: false,
            name: String::new(),
            issuer: String::new(),
            secret: String::new(),
            digits: 0,
            period: 0,
            error: None,
            toast: None,
            pending_delete: None,
            task: None,
        }
    }

    fn toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }

    fn refresh(&mut self) {
        let state = self.totp_state.clone();
        let overview = totp::get_totp_accounts(&state);
        self.accounts = overview.accounts;
        // 钥匙串等错误在页头常驻提示（每秒刷新时保持，不覆盖表单错误）
        if self.load_error.is_none() {
            self.load_error = overview.error;
        }
        self.accounts_at = Some(Instant::now());
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // 每秒刷新验证码
        if self.accounts_at.is_none()
            || self.accounts_at.unwrap().elapsed() >= Duration::from_secs(1)
        {
            self.refresh();
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
                            self.add_open = false;
                            self.refresh();
                        }
                        Job::Copy => self.toast(format!("已复制 {value}")),
                        Job::Delete => {
                            self.pending_delete = None;
                            self.toast("账户已删除");
                            self.refresh();
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

        let palette = Theme::current(&ctx).palette;
        let muted = palette.muted_foreground;
        ui.heading("2FA 验证码");
        ui.add_space(4.0);
        ui.label(RichText::new("管理 TOTP 账户，点击验证码即可复制。").color(muted));
        if let Some(error) = &self.load_error {
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(palette.destructive_foreground.gamma_multiply(0.1))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .stroke(egui::Stroke::new(1.0_f32, palette.destructive))
                .show(ui, |ui| {
                    ui.colored_label(
                        palette.destructive,
                        RichText::new(format!("{error}（验证码暂不可用；授权后重启应用即可恢复）"))
                            .small(),
                    );
                });
        }

        // 工具栏：右侧「添加账户」
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    Button::new("添加账户")
                        .size(ButtonSize::Sm)
                        .icon(&lucide::PLUS),
                )
                .clicked()
            {
                self.error = None;
                self.add_open = true;
            }
        });
        ui.add_space(12.0);

        self.account_grid(ui, &ctx, muted);
        self.add_dialog(&ctx, muted);
        self.draw_toast(&ctx);
    }

    fn account_grid(&mut self, ui: &mut Ui, ctx: &egui::Context, muted: egui::Color32) {
        let mut copy: Option<String> = None;
        let mut confirm_delete: Option<String> = None;

        if self.accounts.is_empty() {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 120.0),
                egui::Sense::hover(),
            );
            ui.painter().rect_stroke(
                rect,
                10,
                egui::Stroke::new(1.0_f32, muted),
                egui::StrokeKind::Outside,
            );
            let icon = &lucide::KEY_ROUND;
            let icon_rect = egui::Rect::from_center_size(
                rect.center_top() + egui::vec2(0.0, 36.0),
                egui::vec2(28.0, 28.0),
            );
            paint_icon(
                ui.painter(),
                icon_rect,
                icon,
                muted,
                Theme::current(ctx).metrics.icon_stroke,
            );
            ui.painter().text(
                rect.center() + egui::vec2(0.0, 28.0),
                egui::Align2::CENTER_CENTER,
                "暂无 2FA 账户",
                egui::FontId::proportional(14.0),
                muted,
            );
        } else {
            egui::Grid::new("totp_cards")
                .num_columns(2)
                .spacing([12.0, 12.0])
                .min_col_width((ui.available_width() - 12.0) / 2.0)
                .show(ui, |ui| {
                    // card_content 需要 &mut self（悬停操作等），用快照避免借用冲突
                    let snapshot = self.accounts.clone();
                    for (n, account) in snapshot.iter().enumerate() {
                        if n > 0 && n % 2 == 0 {
                            ui.end_row();
                        }
                        let card_rect = layout::card(ui, |ui| {
                            self.card_content(
                                ui,
                                ctx,
                                account,
                                muted,
                                &mut copy,
                                &mut confirm_delete,
                            );
                            ui.min_rect()
                        });
                        // 点卡片即复制；同帧点了卡片内按钮（复制/删除/确认）则不触发
                        let button_used = copy.is_some() || confirm_delete.is_some();
                        if !button_used
                            && ui
                                .interact(
                                    card_rect,
                                    egui::Id::new(("totp_card", &account.id)),
                                    egui::Sense::click(),
                                )
                                .clicked()
                        {
                            copy = Some(account.id.clone());
                        }
                    }
                });
        }

        if let Some(id) = copy {
            let totp_state = self.totp_state.clone();
            let clip_state = self.clip_state.clone();
            self.error = None;
            self.task = Some((
                Job::Copy,
                Task::spawn(ctx, move || {
                    totp::copy_totp_code(&totp_state, &clip_state, id)
                }),
            ));
        }
        if let Some(id) = confirm_delete {
            let state = self.totp_state.clone();
            self.error = None;
            self.task = Some((
                Job::Delete,
                Task::spawn(ctx, move || {
                    totp::delete_totp_account(&state, id).map(|_| String::new())
                }),
            ));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn card_content(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        account: &TotpAccountView,
        muted: egui::Color32,
        copy: &mut Option<String>,
        confirm_delete: &mut Option<String>,
    ) {
        let pending = self.pending_delete.as_deref() == Some(account.id.as_str());

        // 标题行：名称 + 发行方，右侧悬停操作
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - 96.0).max(120.0));
                ui.label(RichText::new(&account.name).strong());
                ui.label(
                    RichText::new(if account.issuer.is_empty() {
                        "TOTP"
                    } else {
                        &account.issuer
                    })
                    .small()
                    .color(muted),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let del = Button::new("删除")
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::IconSm)
                    .icon(&lucide::TRASH_2);
                if ui.add(del).on_hover_text("删除账户").clicked() {
                    self.pending_delete = Some(account.id.clone());
                }
                let cp = Button::new("复制")
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::IconSm)
                    .icon(&lucide::COPY);
                if ui.add(cp).on_hover_text("复制").clicked() {
                    *copy = Some(account.id.clone());
                }
            });
        });

        // 大号等宽验证码
        ui.add_space(10.0);
        ui.label(
            RichText::new(display_code(&account.code))
                .monospace()
                .strong()
                .size(28.0),
        );
        ui.add_space(10.0);

        // 剩余时间：图标 + 细进度条 + 秒数
        ui.horizontal(|ui| {
            let timer_rect = ui
                .allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover())
                .0;
            paint_icon(
                ui.painter(),
                timer_rect,
                &lucide::TIMER,
                muted,
                Theme::current(ctx).metrics.icon_stroke,
            );
            let width = ui.available_width() - 44.0;
            ui.add_sized(
                [width, 6.0],
                Progress::new(fraction(account.remaining, account.period)).height(6.0),
            );
            ui.label(
                RichText::new(format!("{} 秒", account.remaining))
                    .small()
                    .color(muted),
            );
        });

        // 行内删除确认条
        if pending {
            ui.add_space(8.0);
            let destructive = Theme::current(ctx).palette.destructive;
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("确定删除「{}」？", account.name))
                        .small()
                        .color(destructive),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            Button::new("确认删除")
                                .variant(ButtonVariant::Destructive)
                                .size(ButtonSize::Sm),
                        )
                        .clicked()
                    {
                        *confirm_delete = Some(account.id.clone());
                    }
                    if ui
                        .add(
                            Button::new("取消")
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm),
                        )
                        .clicked()
                    {
                        self.pending_delete = None;
                    }
                });
            });
        }
    }

    fn add_dialog(&mut self, ctx: &egui::Context, muted: egui::Color32) {
        if !self.add_open {
            return;
        }
        // 粘贴 otpauth 链接时回填名称/发行方，位数/周期隐藏
        let is_otpauth = self.secret.trim().to_lowercase().starts_with("otpauth://");
        let preview = is_otpauth
            .then(|| totp::otpauth_preview(&self.secret))
            .flatten();
        let busy = self.task.is_some();

        let mut open = self.add_open;
        let mut submit = false;
        Dialog::new("totp_add", "添加账户")
            .description("输入 Base32 密钥或 otpauth:// 链接，也可以从二维码识别。")
            .show(ctx, &mut open, |ui| {
                ui.set_min_width(480.0);
                if let Some(error) = &self.error {
                    ui.colored_label(Theme::current(ctx).palette.destructive, error);
                    ui.add_space(6.0);
                }
                ui.label(
                    RichText::new("Base32 密钥或 otpauth:// 链接")
                        .small()
                        .color(muted),
                );
                ui.add_sized(
                    [ui.available_width(), 32.0],
                    Input::new(&mut self.secret).hint("JBSWY3DPEHPK3PXP 或 otpauth://totp/…"),
                );
                ui.add_space(8.0);
                layout::row(ui, 8.0, |ui| {
                    let capture = Button::new("截图识别")
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .icon(&lucide::IMAGE_PLUS)
                        .enabled(!busy);
                    if ui.add(capture).clicked() {
                        self.error = None;
                        self.task = Some((Job::Qr, Task::spawn(ctx, totp::capture_totp_qr)));
                    }
                    let paste = Button::new("读取剪贴板截图")
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .icon(&lucide::CLIPBOARD_PASTE)
                        .enabled(!busy);
                    if ui.add(paste).clicked() {
                        self.error = None;
                        self.task =
                            Some((Job::Qr, Task::spawn(ctx, totp::decode_totp_qr_clipboard)));
                    }
                    let file = Button::new("导入图片")
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .icon(&lucide::FOLDER_OPEN)
                        .enabled(!busy);
                    if ui.add(file).clicked() {
                        self.pick_qr_file(ctx);
                    }
                    if busy {
                        ui.label(RichText::new("正在识别二维码…").small().color(muted));
                    }
                });
                ui.add_space(8.0);

                // 名称 / 发行方（otpauth 时回填为只读）
                let name_hint = if is_otpauth {
                    "已从链接获取"
                } else {
                    "例如：name@example.com"
                };
                let issuer_hint = if is_otpauth {
                    "已从链接获取"
                } else {
                    "可选，例如：GitHub"
                };
                ui.label(
                    RichText::new(format!("账户名称（{name_hint}）"))
                        .small()
                        .color(muted),
                );
                if let Some((name, _, _, _)) = &preview {
                    let mut name = name.clone();
                    ui.add_enabled(false, Input::new(&mut name).hint(name_hint));
                } else {
                    ui.add_sized(
                        [ui.available_width(), 32.0],
                        Input::new(&mut self.name).hint(name_hint),
                    );
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("发行方（{issuer_hint}）"))
                        .small()
                        .color(muted),
                );
                if let Some((_, issuer, _, _)) = &preview {
                    let mut issuer = issuer.clone();
                    ui.add_enabled(false, Input::new(&mut issuer).hint(issuer_hint));
                } else {
                    ui.add_sized(
                        [ui.available_width(), 32.0],
                        Input::new(&mut self.issuer).hint(issuer_hint),
                    );
                }
                ui.add_space(8.0);

                if !is_otpauth {
                    layout::row(ui, 12.0, |ui| {
                        ui.label("位数");
                        ui.add(Select::new("totp_digits", &mut self.digits, &DIGITS));
                        ui.label("刷新周期");
                        ui.add(Select::new("totp_period", &mut self.period, &PERIODS));
                    });
                    ui.add_space(8.0);
                }

                dialog::footer(ui, |ui| {
                    let label = if busy {
                        "添加中…"
                    } else if is_otpauth {
                        "从链接添加"
                    } else {
                        "添加"
                    };
                    submit |= ui
                        .add(
                            Button::new(label)
                                .icon(&lucide::PLUS)
                                .enabled(!busy && !self.secret.trim().is_empty()),
                        )
                        .clicked();
                });
            });

        if !open {
            self.add_open = false;
            self.error = None;
        }
        if submit {
            let name = preview
                .as_ref()
                .map(|p| p.0.clone())
                .unwrap_or_else(|| self.name.clone());
            let issuer = preview
                .as_ref()
                .map(|p| p.1.clone())
                .unwrap_or_else(|| self.issuer.clone());
            let digits = preview.as_ref().map(|p| p.2).unwrap_or([6, 8][self.digits]);
            let period = preview
                .as_ref()
                .map(|p| p.3)
                .unwrap_or([30, 60][self.period]);
            let secret = self.secret.clone();
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
    }

    /// 文件选择器必须在主线程打开（macOS 限制），选择后再到后台解码。
    fn pick_qr_file(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("选择二维码图片")
            .add_filter("图片", &["png", "jpg", "jpeg", "bmp", "webp"])
            .pick_file()
        else {
            return;
        };
        self.task = Some((
            Job::Qr,
            Task::spawn(ctx, move || {
                let bytes = std::fs::read(path).map_err(|e| format!("读取图片失败：{e}"))?;
                totp::decode_qr_bytes(&bytes)
            }),
        ));
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
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-24.0, -24.0))
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

fn fraction(remaining: u64, period: u64) -> f32 {
    if period == 0 {
        return 0.0;
    }
    (remaining as f32 / period as f32) * 100.0
}
