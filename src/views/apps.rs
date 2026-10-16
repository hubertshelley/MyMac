//! 应用卸载页（对应旧版 AppsView.vue）。

use std::collections::HashSet;

use eframe::egui::{self, Color32, RichText, Ui};
use mymac_core::apps::{self, AppInfo, AppRelatedItem};
use mymac_core::event::{AppEvent, EventSink};
use shadcn_egui::components::badge::{Badge, BadgeVariant};
use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
use shadcn_egui::components::checkbox::Checkbox;
use shadcn_egui::components::dialog::{self, Dialog};
use shadcn_egui::components::input::Input;
use shadcn_egui::icons::lucide;
use shadcn_egui::{Theme, layout};

use crate::event::{UiEvent, UiSink};
use crate::task::{Poll, Task};

/// 卸载弹窗状态
struct Uninstall {
    app: AppInfo,
    related: Option<Vec<AppRelatedItem>>,
    scanning: Option<Task<Result<Vec<AppRelatedItem>, String>>>,
    selected: HashSet<String>,
    deleting: Option<Task<Result<(), String>>>,
}

pub struct AppsView {
    apps: Vec<AppInfo>,
    loading: Option<Task<Vec<AppInfo>>>,
    loaded: bool,
    search: String,
    message: Option<String>,
    error: Option<String>,
    uninstall: Option<Uninstall>,
}

impl AppsView {
    pub fn new() -> Self {
        Self {
            apps: Vec::new(),
            loading: None,
            loaded: false,
            search: String::new(),
            message: None,
            error: None,
            uninstall: None,
        }
    }

    /// 后台线程算出某个应用的大小后回调（由主线程 `logic` 转发）。
    pub fn on_app_size(&mut self, info: AppInfo) {
        if let Some(target) = self.apps.iter_mut().find(|a| a.id == info.id) {
            target.size = info.size;
        }
    }

    fn reload(&mut self, ctx: &egui::Context, sink: &UiSink) {
        let sink = sink.clone();
        // core 只认 EventSink；这里把 AppSize 转成 UiEvent，经主线程回到本页。
        let core_sink = EventSink::new(move |event| {
            if let AppEvent::AppSize(info) = event {
                sink.send(UiEvent::AppSize(info));
            }
        });
        self.loading = Some(Task::spawn(ctx, move || apps::list_apps(core_sink)));
    }

    pub fn show(&mut self, ui: &mut Ui, sink: &UiSink) {
        let ctx = ui.ctx().clone();
        if !self.loaded && self.loading.is_none() {
            self.reload(&ctx, sink);
        }
        if let Some(task) = &self.loading {
            match task.poll() {
                Poll::Ready(apps) => {
                    self.apps = apps;
                    self.loaded = true;
                    self.loading = None;
                }
                Poll::Lost => {
                    self.loaded = true;
                    self.loading = None;
                    self.error = Some("扫描应用失败".into());
                }
                Poll::Pending => {}
            }
        }

        let palette = Theme::current(&ctx).palette;
        let muted = palette.muted_foreground;
        ui.heading("应用卸载");
        ui.add_space(12.0);

        layout::row(ui, 12.0, |ui| {
            ui.add_sized(
                [ui.available_width() - 180.0, 32.0],
                Input::new(&mut self.search).hint("按名称或路径搜索应用…"),
            );
            ui.label(RichText::new(format!("共 {} 个可卸载应用", self.apps.len())).color(muted));
        });

        if let Some(message) = &self.message {
            ui.add_space(8.0);
            ui.colored_label(Color32::from_rgb(34, 197, 94), message);
        }
        if let Some(error) = &self.error {
            ui.add_space(8.0);
            ui.colored_label(palette.destructive, error);
        }
        ui.add_space(12.0);

        if self.loading.is_some() && self.apps.is_empty() {
            layout::card(ui, |ui| {
                ui.label(RichText::new("正在扫描应用…").color(muted));
            });
        } else {
            self.list(ui, muted);
        }
        self.uninstall_dialog(&ctx);
    }

    fn list(&mut self, ui: &mut Ui, muted: Color32) {
        let query = self.search.trim().to_lowercase();
        let mut start: Option<usize> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            layout::card(ui, |ui| {
                let mut shown = 0;
                for (i, app) in self.apps.iter().enumerate() {
                    if !query.is_empty()
                        && !app.name.to_lowercase().contains(&query)
                        && !app.path.to_lowercase().contains(&query)
                    {
                        continue;
                    }
                    if shown > 0 {
                        ui.separator();
                    }
                    shown += 1;
                    ui.horizontal(|ui| {
                        avatar(ui, &app.name);
                        ui.vertical(|ui| {
                            ui.set_max_width((ui.available_width() - 110.0).max(120.0));
                            ui.label(RichText::new(&app.name).strong());
                            let version = if app.version.is_empty() {
                                "未知版本"
                            } else {
                                &app.version
                            };
                            let size = if app.size > 0 {
                                crate::fmt::bytes(app.size)
                            } else {
                                "计算中…".to_string()
                            };
                            ui.label(
                                RichText::new(format!("{version} · {size}"))
                                    .small()
                                    .color(muted),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let button = Button::new("卸载")
                                .variant(ButtonVariant::Destructive)
                                .size(ButtonSize::Sm)
                                .icon(&lucide::TRASH_2);
                            if ui.add(button).clicked() {
                                start = Some(i);
                            }
                        });
                    });
                }
                if shown == 0 {
                    ui.label(RichText::new("未找到匹配的应用").color(muted));
                }
            });
        });
        if let Some(i) = start {
            self.begin_uninstall(ui.ctx(), i);
        }
    }

    fn begin_uninstall(&mut self, ctx: &egui::Context, index: usize) {
        let app = self.apps[index].clone();
        let scan_app = app.clone();
        self.error = None;
        self.message = None;
        self.uninstall = Some(Uninstall {
            app,
            related: None,
            scanning: Some(Task::spawn(ctx, move || apps::scan_app_related(scan_app))),
            selected: HashSet::new(),
            deleting: None,
        });
    }

    fn uninstall_dialog(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.uninstall else {
            return;
        };

        // 关联项扫描结果：应用本体默认选中，关联项由用户主动勾选
        if let Some(task) = &state.scanning {
            match task.poll() {
                Poll::Ready(Ok(items)) => {
                    state.selected = items
                        .iter()
                        .filter(|i| i.is_app)
                        .map(|i| i.path.clone())
                        .collect();
                    state.related = Some(items);
                    state.scanning = None;
                }
                Poll::Ready(Err(e)) => {
                    self.error = Some(e);
                    self.uninstall = None;
                    return;
                }
                Poll::Lost => {
                    self.error = Some("查找关联项失败".into());
                    self.uninstall = None;
                    return;
                }
                Poll::Pending => {}
            }
        }
        // 删除结果
        let mut finished: Option<Result<(), String>> = None;
        if let Some(task) = &state.deleting {
            match task.poll() {
                Poll::Ready(result) => finished = Some(result),
                Poll::Lost => finished = Some(Err("卸载任务异常结束".into())),
                Poll::Pending => {}
            }
        }
        if let Some(result) = finished {
            let name = state.app.name.clone();
            match result {
                Ok(()) => {
                    self.message = Some(format!("已处理「{name}」选中的项目"));
                    self.error = None;
                    self.loaded = false; // 重新扫描列表
                }
                Err(e) => {
                    self.error = Some(e);
                    self.message = None;
                }
            }
            self.uninstall = None;
            return;
        }

        let palette = Theme::current(ctx).palette;
        let muted = palette.muted_foreground;
        let title = format!("卸载 {}", state.app.name);
        let busy = state.deleting.is_some();
        let selected_size: u64 = state
            .related
            .iter()
            .flatten()
            .filter(|i| state.selected.contains(&i.path))
            .map(|i| i.size)
            .sum();
        let selected_count = state.selected.len();

        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        Dialog::new("apps_uninstall", &title)
            .description("请选择要移入废纸篓的项目。应用本体已默认选中。")
            .show(ctx, &mut open, |ui| {
                ui.set_min_width(520.0);
                match &state.related {
                    None => {
                        ui.label(RichText::new("正在查找关联项…").color(muted));
                    }
                    Some(items) => {
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(ui, |ui| {
                                for item in items {
                                    let mut checked = state.selected.contains(&item.path);
                                    ui.horizontal(|ui| {
                                        if ui.add(Checkbox::new(&mut checked)).changed() {
                                            if checked {
                                                state.selected.insert(item.path.clone());
                                            } else {
                                                state.selected.remove(&item.path);
                                            }
                                        }
                                        ui.vertical(|ui| {
                                            ui.set_max_width(
                                                (ui.available_width() - 90.0).max(120.0),
                                            );
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new(&item.name).strong());
                                                let variant = if item.is_app {
                                                    BadgeVariant::Default
                                                } else {
                                                    BadgeVariant::Outline
                                                };
                                                ui.add(Badge::new(&item.kind).variant(variant));
                                            });
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&item.path).small().color(muted),
                                                )
                                                .truncate(),
                                            );
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(crate::fmt::bytes(item.size))
                                                        .small()
                                                        .color(muted),
                                                );
                                            },
                                        );
                                    });
                                    ui.add_space(4.0);
                                }
                            });
                    }
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "已选 {selected_count} 项，共 {}",
                        crate::fmt::bytes(selected_size)
                    ))
                    .color(muted),
                );
                dialog::footer(ui, |ui| {
                    let label = if busy {
                        "正在移入废纸篓…"
                    } else {
                        "移入废纸篓"
                    };
                    let enabled = !busy && selected_count > 0 && state.related.is_some();
                    confirm |= ui
                        .add(
                            Button::new(label)
                                .variant(ButtonVariant::Destructive)
                                .enabled(enabled),
                        )
                        .clicked();
                    cancel |= ui
                        .add(
                            Button::new("取消")
                                .variant(ButtonVariant::Outline)
                                .enabled(!busy),
                        )
                        .clicked();
                });
            });

        if confirm && let Some(state) = &mut self.uninstall {
            let paths: Vec<String> = state.selected.iter().cloned().collect();
            state.deleting = Some(Task::spawn(ctx, move || apps::uninstall_app_items(paths)));
        } else if (cancel || !open) && !busy {
            self.uninstall = None;
        }
    }
}

/// 应用名首字母头像，颜色由名称哈希决定（与旧版一致）。
fn avatar(ui: &mut Ui, name: &str) {
    const COLORS: [Color32; 8] = [
        Color32::from_rgb(59, 130, 246),
        Color32::from_rgb(16, 185, 129),
        Color32::from_rgb(249, 115, 22),
        Color32::from_rgb(168, 85, 247),
        Color32::from_rgb(236, 72, 153),
        Color32::from_rgb(20, 184, 166),
        Color32::from_rgb(99, 102, 241),
        Color32::from_rgb(244, 63, 94),
    ];
    let hash = name
        .encode_utf16()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(u32::from(c)));
    let color = COLORS[hash as usize % COLORS.len()];
    let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), color);
    let initial = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(16.0),
        Color32::WHITE,
    );
}
