//! 启动项页（对应旧版 LaunchView.vue）。

use eframe::egui::{self, RichText, Ui};
use mymac_core::launch::{self, LaunchItem};
use shadcn_egui::components::badge::{Badge, BadgeVariant};
use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
use shadcn_egui::components::dialog::{self, Dialog};
use shadcn_egui::components::input::Input;
use shadcn_egui::components::switch::Switch;
use shadcn_egui::icons::lucide;
use shadcn_egui::{Theme, layout};

use crate::task::{Poll, Task};

pub struct LaunchView {
    items: Vec<LaunchItem>,
    loading: Option<Task<Vec<LaunchItem>>>,
    loaded: bool,
    search: String,
    /// 待确认删除的启动项 path
    confirm_delete: Option<(String, String)>,
    /// 最近一次操作的错误提示
    error: Option<String>,
}

impl LaunchView {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            loading: None,
            loaded: false,
            search: String::new(),
            confirm_delete: None,
            error: None,
        }
    }

    fn reload(&mut self, ctx: &egui::Context) {
        self.loading = Some(Task::spawn(ctx, launch::list_launch_items));
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if !self.loaded && self.loading.is_none() {
            self.reload(&ctx);
        }
        if let Some(task) = &self.loading {
            match task.poll() {
                Poll::Ready(items) => {
                    self.items = items;
                    self.loaded = true;
                    self.loading = None;
                }
                Poll::Lost => {
                    self.loading = None;
                    self.loaded = true;
                    self.error = Some("读取启动项失败".into());
                }
                Poll::Pending => {}
            }
        }

        let muted = Theme::current(&ctx).palette.muted_foreground;
        ui.heading("启动项");
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "管理登录时自动启动的项目。用户级登录项可开关或删除，\
                 系统级登录项需要管理员权限，仅支持查看。",
            )
            .color(muted),
        );
        ui.add_space(12.0);

        layout::row(ui, 12.0, |ui| {
            ui.add_sized(
                [ui.available_width() - 120.0, 32.0],
                Input::new(&mut self.search).hint("按名称、程序路径或位置搜索…"),
            );
            let total = self.items.len();
            let text = if self.search.trim().is_empty() {
                format!("共 {total} 项")
            } else {
                format!("{} / {total} 项", self.filtered_indices().len())
            };
            ui.label(RichText::new(text).color(muted));
        });

        if let Some(error) = self.error.clone() {
            ui.add_space(8.0);
            ui.colored_label(Theme::current(&ctx).palette.destructive, error);
        }
        ui.add_space(12.0);

        if self.loading.is_some() && self.items.is_empty() {
            layout::card(ui, |ui| {
                ui.label(RichText::new("正在读取启动项…").color(muted));
            });
        } else {
            self.list(ui, muted);
        }
        self.delete_dialog(&ctx);
    }

    /// 与旧版一致：空格分词，所有词都命中名称/程序/路径/位置才算匹配。
    fn filtered_indices(&self) -> Vec<usize> {
        let terms: Vec<String> = self
            .search
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .collect();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if terms.is_empty() {
                    return true;
                }
                let haystack = [&item.name, &item.program, &item.path, &item.location]
                    .map(|s| s.to_lowercase())
                    .join("\n");
                terms.iter().all(|t| haystack.contains(t))
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn list(&mut self, ui: &mut Ui, muted: egui::Color32) {
        let indices = self.filtered_indices();
        let mut toggle: Option<(usize, bool)> = None;
        let mut reveal: Option<usize> = None;
        let mut delete: Option<usize> = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            layout::card(ui, |ui| {
                if indices.is_empty() {
                    ui.label(RichText::new("没有匹配的启动项").color(muted));
                }
                for (n, &i) in indices.iter().enumerate() {
                    if n > 0 {
                        ui.separator();
                    }
                    let item = &self.items[i];
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_max_width((ui.available_width() - 220.0).max(120.0));
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&item.name).strong());
                                ui.add(Badge::new(&item.location).variant(BadgeVariant::Outline));
                            });
                            let sub = if item.program.is_empty() {
                                &item.path
                            } else {
                                &item.program
                            };
                            ui.add(
                                egui::Label::new(RichText::new(sub).small().color(muted))
                                    .truncate(),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if item.is_user {
                                let del = Button::new("删除")
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Icon)
                                    .icon(&lucide::TRASH_2);
                                if ui.add(del).on_hover_text("删除（移入废纸篓）").clicked()
                                {
                                    delete = Some(i);
                                }
                            }
                            let find = Button::new("在 Finder 中显示")
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Icon)
                                .icon(&lucide::FOLDER_OPEN);
                            if ui.add(find).on_hover_text("在 Finder 中显示").clicked() {
                                reveal = Some(i);
                            }
                            if item.is_user {
                                let mut enabled = item.enabled;
                                if ui.add(Switch::new(&mut enabled)).changed() {
                                    toggle = Some((i, enabled));
                                }
                                ui.label(
                                    RichText::new(if item.enabled {
                                        "已启用"
                                    } else {
                                        "已禁用"
                                    })
                                    .small()
                                    .color(muted),
                                );
                            }
                        });
                    });
                }
            });
        });

        if let Some((i, enabled)) = toggle {
            let path = self.items[i].path.clone();
            match launch::set_launch_item(path, enabled) {
                Ok(()) => {
                    self.items[i].enabled = enabled;
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(i) = reveal {
            let item = &self.items[i];
            // 优先定位实际运行程序，不存在时定位启动项 plist
            let first = if item.program.is_empty() {
                &item.path
            } else {
                &item.program
            };
            if launch::reveal_launch_item(first.clone()).is_err()
                && let Err(e) = launch::reveal_launch_item(item.path.clone())
            {
                self.error = Some(e);
            }
        }
        if let Some(i) = delete {
            let item = &self.items[i];
            self.confirm_delete = Some((item.path.clone(), item.name.clone()));
        }
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some((path, name)) = self.confirm_delete.clone() else {
            return;
        };
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        let description = format!("确定要删除启动项「{name}」吗？它将移入废纸篓。");
        Dialog::new("launch_delete", "删除启动项")
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
            match launch::delete_launch_item(path) {
                Ok(_) => {
                    self.error = None;
                    self.loaded = false; // 触发重新读取
                }
                Err(e) => self.error = Some(e),
            }
            self.confirm_delete = None;
        } else if cancel || !open {
            self.confirm_delete = None;
        }
    }
}
