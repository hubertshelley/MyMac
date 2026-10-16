//! 粘贴板页（对应旧版 ClipboardView.vue）。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{self, ColorImage, RichText, TextureHandle, Ui};
use mymac_core::clipboard::{self, ClipItem, ClipKind, ClipboardState};
use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
use shadcn_egui::components::dialog::{self, Dialog};
use shadcn_egui::components::input::Input;
use shadcn_egui::icons::lucide;
use shadcn_egui::{Theme, layout};

use crate::task::{Poll, Task};

/// 后台动作（剪贴板读写、删除都放线程里，避免阻塞界面）
enum Action {
    Copy(String),
    Delete(String),
    ClearAll,
}

pub struct ClipboardView {
    items: Vec<ClipItem>,
    /// 监听线程报告变化后置位，下一帧重读
    stale: bool,
    search: String,
    toast: Option<(String, Instant)>,
    /// 图片记录的缩略图纹理（按 id 缓存）
    thumbs: HashMap<String, TextureHandle>,
    preview: Option<PreviewImage>,
    confirm_clear: bool,
    task: Option<(Action, Task<Result<(), String>>)>,
}

struct PreviewImage {
    handle: TextureHandle,
    size: [f32; 2],
}

impl ClipboardView {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            stale: true,
            search: String::new(),
            toast: None,
            thumbs: HashMap::new(),
            preview: None,
            confirm_clear: false,
            task: None,
        }
    }

    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    fn reload(&mut self, state: &ClipboardState) {
        self.items = state.items.lock().unwrap().clone();
        self.stale = false;
        // 缩略图缓存跟随记录清理，避免纹理长期驻留
        self.thumbs
            .retain(|id, _| self.items.iter().any(|i| i.id == *id));
    }

    fn spawn(&mut self, ctx: &egui::Context, state: &Arc<ClipboardState>, action: Action) {
        let state = state.clone();
        let job: Box<dyn FnOnce() -> Result<(), String> + Send> = match &action {
            Action::Copy(id) => {
                let id = id.clone();
                Box::new(move || clipboard::copy_clip_item(&state, id))
            }
            Action::Delete(id) => {
                let id = id.clone();
                Box::new(move || clipboard::delete_clip_item(&state, id))
            }
            Action::ClearAll => Box::new(move || clipboard::clear_history(&state)),
        };
        self.task = Some((action, Task::spawn(ctx, job)));
    }

    fn toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }

    pub fn show(&mut self, ui: &mut Ui, state: &Arc<ClipboardState>) {
        if self.stale && self.task.is_none() {
            self.reload(state);
        }
        // 后台动作结果
        if let Some((action, task)) = &self.task {
            match task.poll() {
                Poll::Ready(Ok(())) => {
                    match action {
                        Action::Copy(_) => self.toast("已复制到剪贴板"),
                        Action::Delete(_) => self.toast("已删除该记录"),
                        Action::ClearAll => self.toast("已清空全部记录"),
                    }
                    self.task = None;
                    self.reload(state);
                }
                Poll::Ready(Err(e)) => {
                    self.toast(format!("操作失败：{e}"));
                    self.task = None;
                }
                Poll::Lost => {
                    self.toast("操作异常结束");
                    self.task = None;
                }
                Poll::Pending => {}
            }
        }

        let ctx = ui.ctx().clone();
        let palette = Theme::current(&ctx).palette;
        let muted = palette.muted_foreground;
        ui.heading("粘贴板");
        ui.add_space(12.0);

        // 与旧版一致：文本匹配内容，图片只匹配“图片/image/img”关键词
        let kw = self.search.trim().to_lowercase();
        let filtered: Vec<ClipItem> = self
            .items
            .clone()
            .into_iter()
            .filter(|i| {
                if kw.is_empty() {
                    return true;
                }
                match i.kind {
                    ClipKind::Text => i.content.to_lowercase().contains(&kw),
                    ClipKind::Image => kw == "图片" || kw == "image" || kw == "img",
                }
            })
            .collect();

        layout::row(ui, 12.0, |ui| {
            ui.add_sized(
                [ui.available_width() - 130.0, 32.0],
                Input::new(&mut self.search).hint("搜索粘贴板历史…"),
            );
            let clear = Button::new("清空记录")
                .variant(ButtonVariant::Destructive)
                .size(ButtonSize::Sm)
                .icon(&lucide::CLIPBOARD_X)
                .enabled(!self.items.is_empty());
            if ui.add(clear).clicked() {
                self.confirm_clear = true;
            }
        });
        ui.add_space(12.0);

        let mut copy: Option<String> = None;
        let mut delete: Option<String> = None;
        let mut preview: Option<String> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            layout::card(ui, |ui| {
                if filtered.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(16.0);
                        ui.label(
                            RichText::new(if self.items.is_empty() {
                                "暂无粘贴板历史，复制文本或图片后自动记录"
                            } else {
                                "没有匹配的记录"
                            })
                            .color(muted),
                        );
                        ui.add_space(16.0);
                    });
                }
                for (n, item) in filtered.iter().enumerate() {
                    if n > 0 {
                        ui.separator();
                    }
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_max_width((ui.available_width() - 120.0).max(160.0));
                            match item.kind {
                                ClipKind::Text => {
                                    ui.add(
                                        egui::Label::new(RichText::new(&item.content).small())
                                            .wrap_mode(egui::TextWrapMode::Wrap),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · {} 字符",
                                            item.created_at,
                                            item.content.chars().count()
                                        ))
                                        .small()
                                        .color(muted),
                                    );
                                }
                                ClipKind::Image => {
                                    ui.horizontal(|ui| {
                                        if let Some(handle) = self.thumb(ui.ctx(), item) {
                                            let size = handle.size_vec2();
                                            ui.add(
                                                egui::Image::from_texture(handle)
                                                    .fit_to_exact_size(egui::vec2(
                                                        size.x.min(120.0),
                                                        size.y.min(96.0),
                                                    ))
                                                    .corner_radius(6),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::ZoomIn)
                                            .clicked()
                                            .then(|| preview = Some(item.id.clone()));
                                        }
                                        ui.vertical(|ui| {
                                            let dim = item
                                                .image_size
                                                .map(|(w, h)| format!("图片 {w}×{h}"))
                                                .unwrap_or_else(|| "图片".into());
                                            ui.label(RichText::new(dim).small().color(muted));
                                            ui.label(
                                                RichText::new(&item.created_at)
                                                    .small()
                                                    .color(muted),
                                            );
                                        });
                                    });
                                }
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let del = Button::new("删除")
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::IconSm)
                                .icon(&lucide::TRASH_2);
                            if ui.add(del).on_hover_text("删除").clicked() {
                                delete = Some(item.id.clone());
                            }
                            if item.kind == ClipKind::Image {
                                let view = Button::new("查看原图")
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::IconSm)
                                    .icon(&lucide::MAXIMIZE_2);
                                if ui.add(view).on_hover_text("查看原图").clicked() {
                                    preview = Some(item.id.clone());
                                }
                            }
                            let cp = Button::new("复制")
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::IconSm)
                                .icon(&lucide::COPY);
                            if ui.add(cp).on_hover_text("复制").clicked() {
                                copy = Some(item.id.clone());
                            }
                        });
                    });
                }
            });
        });
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "提示：点击「复制」写回系统剪贴板并置顶；图片记录可查看原图。记录上限 200 条，应用退出后仍会保留。",
            )
            .small()
            .color(muted),
        );

        if let Some(id) = copy {
            self.spawn(&ctx, state, Action::Copy(id));
        }
        if let Some(id) = delete {
            self.spawn(&ctx, state, Action::Delete(id));
        }
        if let Some(id) = preview {
            self.open_preview(&ctx, state, &id);
        }
        self.clear_dialog(&ctx, state);
        self.preview_window(&ctx);
        self.draw_toast(&ctx);
    }

    fn thumb(&mut self, ctx: &egui::Context, item: &ClipItem) -> Option<&TextureHandle> {
        if let Some(file) = &item.thumb_file
            && !self.thumbs.contains_key(&item.id)
            && let Some((rgba, w, h)) = clipboard::load_thumb_image(file)
        {
            let image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            self.thumbs.insert(
                item.id.clone(),
                ctx.load_texture(
                    format!("clip-thumb-{}", item.id),
                    image,
                    egui::TextureOptions::LINEAR,
                ),
            );
        }
        self.thumbs.get(&item.id)
    }

    fn open_preview(&mut self, ctx: &egui::Context, state: &ClipboardState, id: &str) {
        let Some(png) = clipboard::clip_image_png(state, id) else {
            self.toast("读取原图失败");
            return;
        };
        let Ok(image) = image::load_from_memory(&png) else {
            self.toast("原图数据无效");
            return;
        };
        let rgba = image.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let texture = ctx.load_texture(
            format!("clip-full-{id}"),
            ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
            egui::TextureOptions::LINEAR,
        );
        self.preview = Some(PreviewImage {
            size: [w as f32, h as f32],
            handle: texture,
        });
    }

    fn clear_dialog(&mut self, ctx: &egui::Context, state: &Arc<ClipboardState>) {
        if !self.confirm_clear {
            return;
        }
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        Dialog::new("clip_clear", "清空粘贴板历史")
            .description("确定要清空全部粘贴板历史记录吗？")
            .show(ctx, &mut open, |ui| {
                dialog::footer(ui, |ui| {
                    confirm |= ui
                        .add(Button::new("清空").variant(ButtonVariant::Destructive))
                        .clicked();
                    cancel |= ui
                        .add(Button::new("取消").variant(ButtonVariant::Outline))
                        .clicked();
                });
            });
        if confirm {
            self.confirm_clear = false;
            self.spawn(ctx, state, Action::ClearAll);
        } else if cancel || !open {
            self.confirm_clear = false;
        }
    }

    fn preview_window(&mut self, ctx: &egui::Context) {
        let Some(preview) = &self.preview else {
            return;
        };
        let mut open = true;
        egui::Window::new("原图预览")
            .open(&mut open)
            .resizable(true)
            .default_size([640.0, 480.0])
            .show(ctx, |ui| {
                let available = ui.available_size();
                let scale = (available.x / preview.size[0])
                    .min(available.y / preview.size[1])
                    .min(1.0);
                let size = egui::vec2(preview.size[0] * scale, preview.size[1] * scale);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::from_texture(&preview.handle).fit_to_exact_size(size));
                });
            });
        if !open {
            self.preview = None;
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
        egui::Area::new(egui::Id::new("clip-toast"))
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
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}
