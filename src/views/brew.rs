//! Homebrew 管理页（对应旧版 BrewView.vue）。
//!
//! 所有 brew 命令都在 `Task` 后台线程执行；进度经 `EventSink` 写入共享
//! `progress`，由本页渲染。依赖树为懒加载：展开时才查直接依赖，
//! 展开状态按 `kind:name` 键保存在 `tree` 里，整表重载后不丢失。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText, Ui};
use mymac_core::brew::{
    self, BrewOperationResult, BrewPackage, BrewProgress, BrewSource, BrewStatus,
};
use mymac_core::event::{AppEvent, EventSink};
use shadcn_egui::components::badge::{BadgeVariant, badge};
use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
use shadcn_egui::components::dialog::{self, Dialog};
use shadcn_egui::components::icon::{IconView, paint_icon};
use shadcn_egui::components::input::Input;
use shadcn_egui::components::select::Select;
use shadcn_egui::components::tabs::Tabs;
use shadcn_egui::icons::lucide;
use shadcn_egui::{Theme, layout};

use crate::task::{Poll, Task};
use crate::widgets::Progress;

#[derive(Clone, PartialEq)]
enum Busy {
    Load,
    InstallBrew,
    Source,
    Search,
    /// 单个软件操作，参数为 `kind:name`
    Package(String),
    UpgradeAll,
    /// 懒加载直接依赖
    Deps(String),
}

#[derive(Clone, Copy, PartialEq)]
enum PkgAction {
    Install,
    Uninstall,
    Upgrade,
}

enum Job {
    Load,
    InstallBrew,
    Source(BrewSource),
    Search(String),
    Package {
        action: PkgAction,
        name: String,
        kind: String,
    },
    UpgradeAll,
    Deps {
        key: String,
        name: String,
        kind: String,
    },
}

enum Outcome {
    Load(Result<(BrewStatus, Vec<BrewPackage>), String>),
    Message(Result<String, String>),
    Search(Result<Vec<BrewPackage>, String>),
    /// 操作结果 +（更新时）刷新后的软件 +（安装/卸载时）重载后的全量列表
    Package {
        result: Result<(BrewOperationResult, Option<BrewPackage>, Vec<BrewPackage>), String>,
    },
    UpgradeAll(Result<BrewOperationResult, String>),
    Deps {
        key: String,
        result: Result<Vec<BrewPackage>, String>,
    },
}

/// 依赖树节点状态（键 `kind:name`），与软件数据解耦以便整表重载后保留。
struct NodeState {
    expanded: bool,
    loaded: bool,
    children: Vec<String>,
}

pub struct BrewView {
    status: Option<BrewStatus>,
    packages: Vec<BrewPackage>,
    results: Vec<BrewPackage>,
    query: String,
    section: usize,
    started: bool,
    error: Option<String>,
    message: Option<String>,
    output: Option<String>,
    progress: Arc<Mutex<Option<BrewProgress>>>,
    busy: Option<Busy>,
    tree: HashMap<String, NodeState>,
    upgrade_confirm_open: bool,
    task: Option<Task<Outcome>>,
}

impl BrewView {
    pub fn new() -> Self {
        Self {
            status: None,
            packages: Vec::new(),
            results: Vec::new(),
            query: String::new(),
            section: 0,
            started: false,
            error: None,
            message: None,
            output: None,
            progress: Arc::new(Mutex::new(None)),
            busy: None,
            tree: HashMap::new(),
            upgrade_confirm_open: false,
            task: None,
        }
    }

    /// 后台线程用的进度出口：写共享进度并唤醒界面。
    fn sink(&self, ctx: &egui::Context) -> EventSink {
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        EventSink::new(move |event| {
            if let AppEvent::BrewProgress(p) = event {
                *progress.lock().unwrap() = Some(p);
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
        })
    }

    fn spawn(&mut self, ctx: &egui::Context, job: Job) {
        let sink = self.sink(ctx);
        *self.progress.lock().unwrap() = None;
        self.error = None;
        self.message = None;
        self.busy = Some(match &job {
            Job::Load => Busy::Load,
            Job::InstallBrew => Busy::InstallBrew,
            Job::Source(_) => Busy::Source,
            Job::Search(_) => Busy::Search,
            Job::Package { name, kind, .. } => Busy::Package(format!("{kind}:{name}")),
            Job::UpgradeAll => Busy::UpgradeAll,
            Job::Deps { key, .. } => Busy::Deps(key.clone()),
        });
        self.task = Some(Task::spawn(ctx, move || run_job(job, &sink)));
    }

    /// 整表重载后清理已卸载软件的树状态（展开状态按 key 保留）。
    fn prune_tree(&mut self) {
        self.tree
            .retain(|key, _| self.packages.iter().any(|p| p.key() == *key));
    }

    fn apply_package(&mut self, updated: BrewPackage) {
        let key = updated.key();
        if let Some(item) = self.packages.iter_mut().find(|p| p.key() == key) {
            *item = updated.clone();
        }
        if let Some(item) = self.results.iter_mut().find(|p| p.key() == key) {
            *item = updated;
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();

        if !self.started {
            self.started = true;
            self.spawn(&ctx, Job::Load);
        }

        // 后台任务结果
        if let Some(task) = &self.task {
            match task.poll() {
                Poll::Ready(outcome) => {
                    self.apply(&ctx, outcome);
                    self.busy = None;
                    self.task = None;
                }
                Poll::Lost => {
                    self.error = Some("后台任务异常结束".into());
                    self.busy = None;
                    self.task = None;
                }
                Poll::Pending => {}
            }
        }

        let palette = Theme::current(&ctx).palette;
        let muted = palette.muted_foreground;
        let busy = self.busy.clone();

        ui.heading("Homebrew");
        ui.add_space(12.0);

        egui::ScrollArea::vertical()
            .id_salt("brew_scroll")
            .auto_shrink(false)
            .show(ui, |ui| match self.status.clone() {
                None => {
                    ui.label(
                        RichText::new(match self.error.as_deref() {
                            Some(e) => format!("读取 Homebrew 状态失败：{e}"),
                            None => "正在读取 Homebrew 信息…".to_string(),
                        })
                        .color(muted),
                    );
                }
                Some(status) if !status.installed => {
                    self.not_installed_card(ui, &ctx, &busy, muted);
                }
                Some(status) => {
                    self.status_card(ui, &ctx, &status, &busy, muted);
                    ui.add_space(12.0);
                    self.content(ui, &ctx, &busy, muted);

                    let progress = self.progress.lock().unwrap().clone();
                    if let (Some(_), Some(p)) = (&busy, progress) {
                        ui.add_space(12.0);
                        self.progress_card(ui, &p, muted);
                    }
                    ui.add_space(12.0);
                    self.banners(ui, &ctx, muted);
                }
            });

        self.upgrade_dialog(&ctx);
    }

    fn apply(&mut self, ctx: &egui::Context, outcome: Outcome) {
        match outcome {
            Outcome::Load(Ok((status, packages))) => {
                self.status = Some(status);
                self.packages = packages;
                self.prune_tree();
            }
            Outcome::Load(Err(e)) => self.error = Some(e),
            Outcome::Message(Ok(msg)) => self.message = Some(msg),
            Outcome::Message(Err(e)) => self.error = Some(e),
            Outcome::Search(Ok(results)) => {
                self.results = results;
                self.section = 1;
            }
            Outcome::Search(Err(e)) => self.error = Some(e),
            Outcome::Package { result } => match result {
                Ok((op, updated, reloaded)) => {
                    self.message = Some(op.message);
                    self.output = Some(op.output);
                    if let Some(updated) = updated {
                        // 局部刷新：不动树展开状态
                        self.apply_package(updated);
                    }
                    if !reloaded.is_empty() {
                        self.packages = reloaded;
                        self.prune_tree();
                    }
                }
                Err(e) => self.error = Some(e),
            },
            Outcome::UpgradeAll(result) => match result {
                Ok(op) => {
                    self.message = Some(op.message);
                    self.output = Some(op.output);
                    self.spawn(ctx, Job::Load);
                }
                Err(e) => self.error = Some(e),
            },
            Outcome::Deps { key, result } => match result {
                Ok(deps) => {
                    let state = self.tree.entry(key.clone()).or_insert(NodeState {
                        expanded: false,
                        loaded: false,
                        children: Vec::new(),
                    });
                    state.children = deps.iter().map(|p| p.key()).collect();
                    state.loaded = true;
                    state.expanded = !state.children.is_empty();
                }
                Err(e) => self.error = Some(e),
            },
        }
    }

    fn not_installed_card(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        busy: &Option<Busy>,
        muted: egui::Color32,
    ) {
        let disabled = busy.is_some();
        layout::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(IconView::new(&lucide::PACKAGE).size(24.0).color(muted));
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("尚未安装 Homebrew").strong());
                    ui.label(
                        RichText::new(
                            "Homebrew 是 macOS 常用的软件管理工具。安装过程将在系统终端中进行，以便你查看输出并确认管理员授权。",
                        )
                        .small()
                        .color(muted),
                    );
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        Button::new("开始安装")
                            .size(ButtonSize::Sm)
                            .icon(&lucide::DOWNLOAD)
                            .enabled(!disabled),
                    )
                    .clicked()
                {
                    self.spawn(ctx, Job::InstallBrew);
                }
                if ui
                    .add(
                        Button::new("安装完成，刷新状态")
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Sm)
                            .icon(&lucide::REFRESH_CW)
                            .enabled(!disabled),
                    )
                    .clicked()
                {
                    self.spawn(ctx, Job::Load);
                }
            });
        });
    }

    fn status_card(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        status: &BrewStatus,
        busy: &Option<Busy>,
        muted: egui::Color32,
    ) {
        let disabled = busy.is_some();
        let outdated_count = self
            .packages
            .iter()
            .filter(|p| p.top_level && p.outdated && p.trusted)
            .count();

        // 软件源下拉的选中项（任务失败时 status.source 未变，下一帧自动回显）
        let mut source_sel = BrewSource::ALL
            .iter()
            .position(|s| *s == status.source)
            .unwrap_or(0);

        layout::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Homebrew 已就绪").strong());
                        badge(
                            ui,
                            if status.version.is_empty() {
                                "版本未知"
                            } else {
                                &status.version
                            },
                            BadgeVariant::Secondary,
                        );
                    });
                    ui.label(RichText::new(&status.path).small().color(muted));
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            Button::new("更新全部")
                                .size(ButtonSize::Sm)
                                .icon(&lucide::UPLOAD)
                                .enabled(!disabled),
                        )
                        .clicked()
                    {
                        if outdated_count == 0 {
                            self.message = Some("当前没有需要更新的顶层软件".into());
                            self.error = None;
                        } else {
                            self.upgrade_confirm_open = true;
                        }
                    }
                    if outdated_count > 0 {
                        badge(ui, &outdated_count.to_string(), BadgeVariant::Secondary);
                    }
                    if ui
                        .add(
                            Button::new("刷新")
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Sm)
                                .icon(&lucide::REFRESH_CW)
                                .enabled(!disabled),
                        )
                        .clicked()
                    {
                        self.spawn(ctx, Job::Load);
                    }
                    ui.label(RichText::new("软件源").color(muted));
                    let options = [
                        BrewSource::Official.label(),
                        BrewSource::Tsinghua.label(),
                        BrewSource::Ustc.label(),
                    ];
                    let select =
                        Select::new("brew_source", &mut source_sel, &options).enabled(!disabled);
                    if ui.add(select).changed() {
                        let source = BrewSource::ALL[source_sel];
                        if source != status.source {
                            self.spawn(ctx, Job::Source(source));
                        }
                    }
                });
            });
        });
    }

    fn content(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        busy: &Option<Busy>,
        muted: egui::Color32,
    ) {
        let roots: Vec<String> = self
            .packages
            .iter()
            .filter(|p| p.top_level)
            .map(|p| p.key())
            .collect();

        let labels = [
            format!("已安装（{} 个顶层软件）", roots.len()),
            "搜索软件".to_string(),
        ];
        ui.add(Tabs::new(&mut self.section, &[&labels[0], &labels[1]]));
        ui.add_space(8.0);

        if self.section == 1 {
            ui.horizontal(|ui| {
                let width = ui.available_width() - 90.0;
                ui.add_sized(
                    [width, 32.0],
                    Input::new(&mut self.query).hint("搜索命令行软件和图形应用…"),
                );
                if ui
                    .add(
                        Button::new("搜索")
                            .size(ButtonSize::Sm)
                            .icon(&lucide::SEARCH)
                            .enabled(busy.is_none()),
                    )
                    .clicked()
                {
                    let query = self.query.trim().to_string();
                    self.spawn(ctx, Job::Search(query));
                }
            });
            ui.add_space(8.0);
        }

        layout::card(ui, |ui| {
            if self.section == 0 {
                let items = self.visible_nodes(&roots);
                if items.is_empty() {
                    ui.add_space(24.0);
                    ui.label(RichText::new("暂无已安装软件").color(muted));
                    ui.add_space(24.0);
                    return;
                }
                for (key, depth) in items {
                    if let Some(pkg) = self.packages.iter().find(|p| p.key() == key) {
                        let pkg = pkg.clone();
                        self.package_row(ui, ctx, &pkg, depth, true, busy, muted);
                    }
                }
            } else {
                if self.results.is_empty() {
                    ui.add_space(24.0);
                    ui.label(RichText::new("输入名称搜索软件").color(muted));
                    ui.add_space(24.0);
                    return;
                }
                let snapshot: Vec<BrewPackage> = self.results.clone();
                for pkg in &snapshot {
                    self.package_row(ui, ctx, pkg, 0, false, busy, muted);
                }
            }
        });
    }

    /// 深度优先展开后的可见节点（key, depth）。
    fn visible_nodes(&self, roots: &[String]) -> Vec<(String, usize)> {
        let mut visible = Vec::new();
        fn walk(
            key: &str,
            depth: usize,
            tree: &HashMap<String, NodeState>,
            visible: &mut Vec<(String, usize)>,
        ) {
            visible.push((key.to_string(), depth));
            if let Some(state) = tree.get(key)
                && state.expanded
                && state.loaded
            {
                for child in &state.children {
                    walk(child, depth + 1, tree, visible);
                }
            }
        }
        for root in roots {
            walk(root, 0, &self.tree, &mut visible);
        }
        visible
    }

    #[allow(clippy::too_many_arguments)]
    fn package_row(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        pkg: &BrewPackage,
        depth: usize,
        installed_section: bool,
        busy: &Option<Busy>,
        muted: egui::Color32,
    ) {
        let key = pkg.key();
        let disabled = busy.is_some();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            // 展开箭头（仅已安装区的 formula）
            if installed_section && pkg.kind == "formula" {
                let state = self.tree.get(&key);
                let can_expand = match state {
                    None => true,
                    Some(s) => !s.loaded || !s.children.is_empty(),
                };
                if can_expand {
                    let loading = *busy == Some(Busy::Deps(key.clone()));
                    let expanded = state.is_some_and(|s| s.expanded);
                    let icon = if loading {
                        &lucide::LOADER_CIRCLE
                    } else if expanded {
                        &lucide::CHEVRON_DOWN
                    } else {
                        &lucide::CHEVRON_RIGHT
                    };
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                    paint_icon(
                        ui.painter(),
                        rect,
                        icon,
                        muted,
                        Theme::current(ui.ctx()).metrics.icon_stroke,
                    );
                    if response.clicked() && !disabled && !loading {
                        if expanded {
                            if let Some(s) = self.tree.get_mut(&key) {
                                s.expanded = false;
                            }
                        } else {
                            self.spawn(
                                ctx,
                                Job::Deps {
                                    key,
                                    name: pkg.name.clone(),
                                    kind: pkg.kind.clone(),
                                },
                            );
                        }
                    }
                } else {
                    ui.add_space(20.0);
                }
            } else if installed_section {
                ui.add_space(20.0);
            }
            if depth > 0 {
                ui.add_space((depth * 20) as f32);
            }

            // 名称与徽标
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&pkg.name).strong().size(13.0));
                    badge(
                        ui,
                        if pkg.kind == "cask" {
                            "图形应用"
                        } else {
                            "命令行"
                        },
                        BadgeVariant::Outline,
                    );
                    if installed_section && !pkg.top_level {
                        badge(ui, "依赖", BadgeVariant::Secondary);
                    }
                    if !pkg.trusted {
                        badge(ui, "来源未信任", BadgeVariant::Destructive);
                    }
                    if pkg.outdated {
                        badge(ui, "可更新", BadgeVariant::Secondary);
                    } else if pkg.installed && !installed_section {
                        badge(ui, "已安装", BadgeVariant::Secondary);
                    }
                });
                if !pkg.version.is_empty() {
                    ui.label(RichText::new(&pkg.version).small().color(muted));
                }
                if !pkg.trusted {
                    ui.label(
                        RichText::new(format!(
                            "{} 尚未获得 Homebrew 信任，无法读取更新信息",
                            pkg.tap.as_deref().unwrap_or("第三方来源")
                        ))
                        .small()
                        .color(muted),
                    );
                }
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pkg.installed {
                    if pkg.outdated
                        && ui
                            .add(
                                Button::new("更新")
                                    .variant(ButtonVariant::Outline)
                                    .size(ButtonSize::Sm)
                                    .icon(&lucide::UPLOAD)
                                    .enabled(!disabled),
                            )
                            .clicked()
                    {
                        self.spawn(
                            ctx,
                            Job::Package {
                                action: PkgAction::Upgrade,
                                name: pkg.name.clone(),
                                kind: pkg.kind.clone(),
                            },
                        );
                    }
                    if ui
                        .add(
                            Button::new("卸载")
                                .variant(ButtonVariant::Destructive)
                                .size(ButtonSize::Sm)
                                .icon(&lucide::TRASH_2)
                                .enabled(!disabled),
                        )
                        .clicked()
                    {
                        self.spawn(
                            ctx,
                            Job::Package {
                                action: PkgAction::Uninstall,
                                name: pkg.name.clone(),
                                kind: pkg.kind.clone(),
                            },
                        );
                    }
                } else if ui
                    .add(
                        Button::new("安装")
                            .size(ButtonSize::Sm)
                            .icon(&lucide::DOWNLOAD)
                            .enabled(!disabled),
                    )
                    .clicked()
                {
                    self.spawn(
                        ctx,
                        Job::Package {
                            action: PkgAction::Install,
                            name: pkg.name.clone(),
                            kind: pkg.kind.clone(),
                        },
                    );
                }
            });
        });
        ui.add_space(6.0);
        ui.separator();
    }

    fn progress_card(&mut self, ui: &mut Ui, p: &BrewProgress, muted: egui::Color32) {
        layout::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&p.stage).strong().size(13.0));
                        if let Some(item) = &p.item {
                            ui.label(RichText::new(item).small().color(muted));
                        }
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(if p.total > 1 {
                            format!("{} / {}", p.current, p.total)
                        } else {
                            format!("{}%", p.percent)
                        })
                        .small()
                        .color(muted),
                    );
                });
            });
            ui.add(Progress::new(p.percent as f32));
        });
    }

    fn banners(&mut self, ui: &mut Ui, ctx: &egui::Context, muted: egui::Color32) {
        let palette = Theme::current(ctx).palette;
        if let Some(message) = &self.message {
            ui.label(RichText::new(message).color(palette.primary));
        }
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(palette.destructive));
        }
        if let Some(output) = &self.output {
            egui::CollapsingHeader::new(RichText::new("查看最近一次操作输出").small().color(muted))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(output).monospace().small());
                        });
                });
        }
    }

    fn upgrade_dialog(&mut self, ctx: &egui::Context) {
        if !self.upgrade_confirm_open {
            return;
        }
        let targets: Vec<String> = self
            .packages
            .iter()
            .filter(|p| p.top_level && p.outdated && p.trusted)
            .map(|p| p.name.clone())
            .collect();

        let mut open = self.upgrade_confirm_open;
        let mut confirm = false;
        let mut cancel = false;
        Dialog::new("brew_upgrade_all", "确认更新顶层软件")
            .description("将更新以下顶层软件，仅作为依赖安装的软件不会单独更新。")
            .show(ctx, &mut open, |ui| {
                ui.set_min_width(420.0);
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(2.0);
                            for name in &targets {
                                badge(ui, name, BadgeVariant::Outline);
                            }
                        });
                    });
                ui.add_space(8.0);
                dialog::footer(ui, |ui| {
                    confirm |= ui
                        .add(
                            Button::new("开始更新")
                                .size(ButtonSize::Sm)
                                .icon(&lucide::UPLOAD),
                        )
                        .clicked();
                    cancel |= ui
                        .add(
                            Button::new("取消")
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Sm),
                        )
                        .clicked();
                });
            });
        if !open || cancel {
            self.upgrade_confirm_open = false;
        }
        if confirm {
            self.upgrade_confirm_open = false;
            self.spawn(ctx, Job::UpgradeAll);
        }
    }
}

/// 各任务的后台执行体（阻塞调用都发生在这里）。
fn run_job(job: Job, sink: &EventSink) -> Outcome {
    match job {
        Job::Load => {
            let status = brew::get_brew_status();
            let packages = if status.installed {
                brew::list_brew_packages(sink).unwrap_or_default()
            } else {
                Vec::new()
            };
            Outcome::Load(Ok((status, packages)))
        }
        Job::InstallBrew => Outcome::Message(brew::start_brew_install()),
        Job::Source(source) => Outcome::Message(brew::set_brew_source(sink, source)),
        Job::Search(query) => Outcome::Search(brew::search_brew_packages(query)),
        Job::Package { action, name, kind } => {
            let op = match action {
                PkgAction::Install => brew::install_brew_package(sink, name.clone(), kind.clone()),
                PkgAction::Uninstall => {
                    brew::uninstall_brew_package(sink, name.clone(), kind.clone())
                }
                PkgAction::Upgrade => brew::upgrade_brew_package(sink, name.clone(), kind.clone()),
            };
            let result = op.and_then(|op| {
                if action == PkgAction::Upgrade {
                    let updated = brew::refresh_brew_package(name, kind)?;
                    Ok((op, Some(updated), Vec::new()))
                } else {
                    let packages = brew::list_brew_packages(sink)?;
                    Ok((op, None, packages))
                }
            });
            Outcome::Package { result }
        }
        Job::UpgradeAll => Outcome::UpgradeAll(brew::upgrade_all_brew_packages(sink)),
        Job::Deps { key, name, kind } => Outcome::Deps {
            key,
            result: brew::get_brew_dependencies(name, kind),
        },
    }
}
