// M0 PoC：验证 egui/eframe 迁移的核心风险项。
//
// 覆盖验证点：
//   1. wgpu(Metal) 渲染下 eframe 窗口正常创建
//   2. 主窗口关闭后隐藏 + Dock 图标隐藏，托盘菜单「打开主面板」可唤回窗口并恢复 Dock
//   3. 全局快捷键 F1（global-hotkey）在主窗口隐藏后仍可触发
//      —— 关键机制：eframe 0.36 起 App 拆为 logic/ui；窗口隐藏时不跑 ui，
//         只有被 request_repaint 唤醒的 logic 会执行，事件处理必须放 logic。
//   4. 后台线程通过通道通知 UI 刷新（方案第 3.2 节的事件模型雏形）
//   5. 中文字体（系统 PingFang）+ egui-phosphor 图标渲染
//   6. 全屏透明、置顶、无边框的截图覆盖层视口（多显示器场景需人工目测）
//
// 运行：在 poc/egui-poc 目录下 cargo run

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use eframe::egui;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

mod macos;

/// UI 事件：后台（托盘菜单 / 全局快捷键）→ 界面
#[derive(Debug, Clone, Copy, PartialEq)]
enum AppEvent {
    ShowMainWindow,
    ToggleOverlay,
    Quit,
}

fn overlay_id() -> egui::ViewportId {
    egui::ViewportId(egui::Id::new("overlay"))
}

/// 后台线程与 eframe 之间共享的 egui::Context（创建后由 creator 回调填充），
/// 供 forwarder 在发事件后 request_repaint 唤醒隐藏状态的 logic。
type SharedCtx = Arc<Mutex<Option<egui::Context>>>;

struct PocApp {
    receiver: mpsc::Receiver<AppEvent>,
    event_tx: Arc<mpsc::Sender<AppEvent>>,
    shared_ctx: SharedCtx,
    /// 覆盖层是否显示；覆盖层自己关闭时也会改写，所以用原子量共享。
    overlay_open: Arc<AtomicBool>,
    last_hotkey: Option<std::time::Instant>,
}

fn poc_log(msg: &str) {
    eprintln!("[poc] {msg}");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/mymac-poc.log")
    {
        use std::io::Write;
        let _ = writeln!(f, "{msg}");
    }
}

fn main() -> eframe::Result {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();
    let _ = std::fs::write("/tmp/mymac-poc.log", format!("==== 启动 {} ====\n", chrono_now()));
    poc_log("main 开始");

    let (tx, rx) = mpsc::channel::<AppEvent>();
    let tx = Arc::new(tx);
    let shared_ctx: SharedCtx = Arc::default();

    // ---- 托盘 + 全局快捷键（必须在主线程创建；事件循环由 eframe/winit 驱动 NSApp）----
    let tray = match setup_tray() {
        Ok(t) => {
            poc_log("托盘创建成功");
            t
        }
        Err(error) => {
            poc_log(&format!("创建托盘失败：{error}"));
            return Err(eframe::Error::AppCreation(error));
        }
    };
    let hotkey_manager = match setup_hotkey() {
        Ok(m) => {
            poc_log("F1 注册成功");
            m
        }
        Err(error) => {
            poc_log(&format!("注册全局快捷键失败：{error}"));
            return Err(eframe::Error::AppCreation(Box::new(error)));
        }
    };

    spawn_event_forwarder(tx.clone(), shared_ctx.clone());

    let app = PocApp {
        receiver: rx,
        event_tx: tx,
        shared_ctx: shared_ctx.clone(),
        overlay_open: Arc::new(AtomicBool::new(false)),
        last_hotkey: None,
    };

    let ctx_for_creator = shared_ctx.clone();
    poc_log("进入 run_native");
    let result = eframe::run_native(
        "mymac-egui-poc",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("MyMac egui PoC")
                .with_inner_size([560.0, 420.0])
                .with_icon(solid_icon_data(64, 64, [40, 120, 220, 255])),
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(move |cc| {
            poc_log("creator 回调执行");
            install_fonts(&cc.egui_ctx);
            *ctx_for_creator.lock().unwrap() = Some(cc.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    );

    poc_log(&format!("run_native 返回：{result:?}"));
    drop(tray);
    drop(hotkey_manager);
    result
}

/// 简易时间戳（避免引入 chrono，PoC 只用于日志）
fn chrono_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

fn forward(tx: &mpsc::Sender<AppEvent>, shared_ctx: &SharedCtx, event: AppEvent) {
    if tx.send(event).is_ok()
        && let Some(ctx) = shared_ctx.lock().unwrap().as_ref()
    {
        // 必须显式唤醒 ROOT：在覆盖层回调里调用 request_repaint() 只会重绘覆盖层自己，
        // 而 App::logic 只在 ROOT 视口执行，事件会一直滞留在通道里。
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// 把 muda 菜单事件、托盘点击、global-hotkey 事件统一转发到 AppEvent 通道。
fn spawn_event_forwarder(tx: Arc<mpsc::Sender<AppEvent>>, shared_ctx: SharedCtx) {
    // muda 菜单事件
    {
        let (tx, shared_ctx) = (tx.clone(), shared_ctx.clone());
        std::thread::spawn(move || loop {
            if let Ok(event) = MenuEvent::receiver().recv()
                && let Some(ev) = match event.id().as_ref() {
                    "show" => Some(AppEvent::ShowMainWindow),
                    "overlay" => Some(AppEvent::ToggleOverlay),
                    "quit" => Some(AppEvent::Quit),
                    _ => None,
                }
            {
                forward(&tx, &shared_ctx, ev);
            }
        });
    }
    // 托盘左键点击 → 唤回主窗口
    {
        let (tx, shared_ctx) = (tx.clone(), shared_ctx.clone());
        std::thread::spawn(move || loop {
            if let Ok(event) = TrayIconEvent::receiver().recv()
                && let TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } = event
            {
                forward(&tx, &shared_ctx, AppEvent::ShowMainWindow);
            }
        });
    }
    // 全局快捷键 F1
    std::thread::spawn(move || loop {
        if let Ok(event) = GlobalHotKeyEvent::receiver().recv()
            && event.state() == HotKeyState::Pressed
        {
            poc_log(&format!("收到全局快捷键事件 id={}", event.id()));
            forward(&tx, &shared_ctx, AppEvent::ToggleOverlay);
        }
    });
}

fn setup_tray() -> Result<TrayIcon, Box<dyn std::error::Error + Send + Sync>> {
    let show_item = MenuItem::with_id("show", "打开主面板", true, None);
    let overlay_item = MenuItem::with_id("overlay", "测试透明覆盖层（F1）", true, None);
    let quit_item = MenuItem::with_id("quit", "退出 PoC", true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &show_item,
        &overlay_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ])?;

    let (rgba, w, h) = solid_icon(32, 32, [40, 120, 220, 255]);
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("MyMac egui PoC")
        .with_icon(tray_icon::Icon::from_rgba(rgba, w, h)?)
        .build()?;
    Ok(tray)
}

fn setup_hotkey() -> global_hotkey::Result<GlobalHotKeyManager> {
    let manager = GlobalHotKeyManager::new()?;
    manager.register(HotKey::new(None, Code::F1))?;
    // 对照组：带修饰键的组合，用来区分“F1 本身被系统占用”和“机制不通”
    manager.register(HotKey::new(Some(Modifiers::CONTROL), Code::F1))?;
    Ok(manager)
}

impl eframe::App for PocApp {
    // 窗口隐藏时唯一会继续执行的方法：在这里消费后台事件并驱动状态。
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                AppEvent::ShowMainWindow => {
                    macos::set_dock_visible(true);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    poc_log("托盘唤回主窗口");
                }
                AppEvent::ToggleOverlay => {
                    self.last_hotkey = Some(std::time::Instant::now());
                    let open = !self.overlay_open.load(Ordering::SeqCst);
                    self.overlay_open.store(open, Ordering::SeqCst);
                    // 覆盖层视口在 ui 首帧已创建（隐藏态）；这里只切可见性。
                    // 主窗口隐藏时 ui 不跑，logic 是唯一能驱动它的入口。
                    ctx.send_viewport_cmd_to(overlay_id(), egui::ViewportCommand::Visible(open));
                    if open {
                        ctx.send_viewport_cmd_to(overlay_id(), egui::ViewportCommand::Focus);
                    }
                    poc_log(&format!(
                        "ToggleOverlay（F1/托盘）→ 覆盖层 {}",
                        if open { "显示" } else { "隐藏" }
                    ));
                }
                AppEvent::Quit => {
                    poc_log("退出（托盘菜单）");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    // 窗口可见时每帧调用。
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // 拦截主窗口关闭：取消关闭 → 隐藏窗口 + 隐藏 Dock（托盘常驻）
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            macos::set_dock_visible(false);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            poc_log("主窗口已隐藏，托盘常驻");
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("MyMac egui PoC 验证面板");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label(egui_phosphor::regular::CAMERA.to_string());
                ui.label("F1 切换透明覆盖层（全局快捷键）");
            });
            ui.horizontal(|ui| {
                ui.label(egui_phosphor::regular::TRAY.to_string());
                ui.label("托盘左键 / 菜单「打开主面板」唤回窗口");
            });
            ui.separator();

            if let Some(t) = self.last_hotkey {
                ui.label(format!(
                    "最近一次 F1 触发：{:.1} 秒前",
                    t.elapsed().as_secs_f32()
                ));
            } else {
                ui.label("尚未触发过 F1");
            }

            ui.add_space(8.0);
            ui.label("验证操作清单：");
            ui.label("1. 点击窗口关闭按钮 → 窗口与 Dock 图标消失，托盘仍在");
            ui.label("2. 托盘左键或菜单「打开主面板」→ 窗口与 Dock 恢复");
            ui.label("3. 先关闭主窗口，再在任意应用前台按 F1 → 覆盖层应照常出现");
            ui.label("4. 覆盖层内输入中文验证 IME；Esc 或点击按钮关闭覆盖层");

            ui.add_space(8.0);
            if ui
                .button(format!(
                    "{} 手动开关透明覆盖层",
                    egui_phosphor::regular::SIDEBAR_SIMPLE
                ))
                .clicked()
            {
                forward(&self.event_tx, &self.shared_ctx, AppEvent::ToggleOverlay);
            }
        });

        // 覆盖层视口：首帧即注册（隐藏态），显隐由 logic 切换
        show_overlay(&ctx, self.overlay_open.clone());
    }
}

fn show_overlay(parent: &egui::Context, overlay_open: Arc<AtomicBool>) {
    let builder = egui::ViewportBuilder::default()
        .with_title("screenshot-overlay")
        .with_maximized(true)
        .with_visible(false) // 初始隐藏，显隐全部靠 ViewportCommand
        .with_transparent(true)
        .with_decorations(false)
        .with_always_on_top()
        .with_resizable(false);

    parent.show_viewport_deferred(overlay_id(), builder, move |ui: &mut egui::Ui, _class| {
        let ctx = ui.ctx().clone();
        let mut close = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgba_unmultiplied(
                200, 30, 30, 60,
            )))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(64.0);
                    ui.heading(
                        egui::RichText::new("透明置顶覆盖层 验证视口")
                            .color(egui::Color32::WHITE)
                            .size(28.0),
                    );
                    ui.label(
                        egui::RichText::new("检查：透明度 / 置顶 / 多显示器 / 中文输入")
                            .color(egui::Color32::WHITE),
                    );
                    if ui.button("关闭覆盖层").clicked() {
                        close = true;
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if close {
            // 覆盖层自管关闭：不依赖被它遮挡的主窗口，本 pass 内直接隐藏自己。
            overlay_open.store(false, Ordering::SeqCst);
            ctx.send_viewport_cmd_to(overlay_id(), egui::ViewportCommand::Visible(false));
            poc_log("覆盖层自行关闭（按钮/Esc）");
        }
    });
}

/// 中文字体：加载系统苹方（ttc 默认取第 0 个 face），并注册 phosphor 图标字体
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let candidates = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
    ];
    for path in candidates {
        match std::fs::read(path) {
            Ok(data) => {
                fonts
                    .font_data
                    .insert("cjk".into(), egui::FontData::from_owned(data).into());
                if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                    family.insert(0, "cjk".into());
                }
                if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                    family.push("cjk".into());
                }
                log::info!("中文字体已加载：{path}");
                break;
            }
            Err(error) => log::warn!("中文字体 {path} 加载失败：{error}"),
        }
    }

    // phosphor 图标字体（注册在默认字体之后，作为图标回退）
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
}

fn solid_icon(w: u32, h: u32, [r, g, b, a]: [u8; 4]) -> (Vec<u8>, u32, u32) {
    let rgba = std::iter::repeat_n([r, g, b, a], (w * h) as usize)
        .flatten()
        .collect();
    (rgba, w, h)
}

fn solid_icon_data(w: u32, h: u32, color: [u8; 4]) -> egui::IconData {
    let (rgba, width, height) = solid_icon(w, h, color);
    egui::IconData {
        width,
        height,
        rgba,
    }
}
