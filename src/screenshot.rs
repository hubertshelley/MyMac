//! 截图覆盖层：每显示器一个全屏置顶透明视口 + 冻结背景纹理。
//!
//! 交互（M5）：
//! - 悬停高亮应用窗口；悬停无窗口区域高亮整屏（M5 新需求 1）
//! - 单击选中（窗口 / 整屏），拖拽自由框选，8 控制点调整大小，整体拖动
//! - 标注：矩形、箭头、文本（fontdue 离线栅格化），颜色 / 粗细 / 撤销
//! - 输出：Enter / 复制按钮合成选区写剪贴板；保存文件（rfd）；识别文字（Vision OCR）
//! - 覆盖层存在时再触发（F1 / 托盘）→ 重新抓屏替换会话（M5 新需求 2）
//!
//! 贴图（M6）：置顶小窗，拖动移动、滚轮缩放、右键关闭；独立于覆盖层常驻。

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};
use mymac_core::screenshot::capture::{self, DisplayInfo};
use mymac_core::screenshot::winlist::{self, CaptureWindow};

/// 追加一行诊断日志到 ~/Library/Logs/MyMac.log（.app 方式启动时 stderr 不可见）。
pub fn diag(msg: impl AsRef<str>) {
    let text = msg.as_ref();
    log::info!("{text}");
    if let Some(home) = std::env::var_os("HOME") {
        let path = std::path::Path::new(&home).join("Library/Logs/MyMac.log");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use std::io::Write;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default();
            let _ = writeln!(f, "[{now}] {text}");
        }
    }
}

/// 标注颜色板
const COLORS: [[u8; 4]; 5] = [
    [229, 72, 77, 255],
    [245, 165, 36, 255],
    [48, 164, 108, 255],
    [59, 130, 246, 255],
    [255, 255, 255, 255],
];
/// 标注粗细档位（逻辑像素）
const THICKNESSES: [f32; 3] = [2.0, 4.0, 6.0];

/// 单个显示器的冻结背景。
#[derive(Clone)]
pub struct Tile {
    pub display_id: u32,
    /// 全局逻辑坐标（与 CG / winit 全局坐标一致，左上原点）
    pub rect: egui::Rect,
    pub scale: f64,
    /// 物理像素 RGBA，用于选区裁剪与合成
    pub rgba: Vec<u8>,
    pub px_width: usize,
    pub px_height: usize,
    pub texture: egui::TextureHandle,
}

impl Tile {
    pub fn viewport_id(&self) -> egui::ViewportId {
        viewport_id(self.display_id)
    }
}

fn viewport_id(display_id: u32) -> egui::ViewportId {
    egui::ViewportId(egui::Id::new(("screenshot-overlay", display_id)))
}

fn pin_viewport_id(pin_id: u32) -> egui::ViewportId {
    egui::ViewportId(egui::Id::new(("screenshot-pin", pin_id)))
}

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Select,
    Rect,
    Arrow,
    Text,
}

#[derive(Clone, Copy, PartialEq)]
enum Handle {
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
}

#[derive(Clone)]
struct Shape {
    tool: Tool,
    /// 覆盖范围（全局逻辑坐标）
    rect: egui::Rect,
    text: String,
    color: [u8; 4],
    width: f32,
}

/// 贴图：置顶小窗，独立于截图覆盖层常驻（关闭覆盖层后仍显示）。
struct PinImage {
    id: u32,
    texture: egui::TextureHandle,
    /// 窗口位置（全局逻辑坐标）
    pos: egui::Pos2,
    /// 显示尺寸（逻辑像素，滚轮缩放改变）
    size: egui::Vec2,
    /// 拖动起点（全局逻辑坐标）
    drag_ref: Option<egui::Pos2>,
}

/// 标注文字的离线栅格化（共享一次字体加载）。
struct TextRaster {
    font: fontdue::Font,
}

impl TextRaster {
    fn new() -> Option<Self> {
        let bytes = std::fs::read("/System/Library/Fonts/SFNS.ttf")
            .or_else(|_| std::fs::read("/System/Library/Fonts/Helvetica.ttc"))
            .or_else(|_| std::fs::read("/System/Library/Fonts/Monaco.ttf"))
            .ok()?;
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()?;
        Some(Self { font })
    }
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        rgba: &mut [u8],
        px_w: usize,
        px_h: usize,
        x: i32,
        y: i32,
        text: &str,
        size: f32,
        color: [u8; 4],
    ) {
        let mut pen = x;
        for ch in text.chars() {
            let (metrics, bitmap) = self.font.rasterize(ch, size);
            for (index, coverage) in bitmap.iter().enumerate() {
                if *coverage == 0 {
                    continue;
                }
                let gx = pen + (index % metrics.width) as i32;
                let gy = y + metrics.ymin + (index / metrics.width) as i32;
                if gx < 0 || gy < 0 || gx as usize >= px_w || gy as usize >= px_h {
                    continue;
                }
                let base = ((gy as usize) * px_w + gx as usize) * 4;
                let alpha = (*coverage as u32 * color[3] as u32 / 255) as u8;
                for channel in 0..3 {
                    rgba[base + channel] = ((color[channel] as u16 * alpha as u16
                        + rgba[base + channel] as u16 * (255 - alpha) as u16)
                        / 255) as u8;
                }
                rgba[base + 3] = 255;
            }
            pen += metrics.advance_width.round() as i32;
        }
    }
}

/// 覆盖层共享状态（主线程与各视口回调之间共享）。
struct Overlay {
    visible: bool,
    tiles: Vec<Tile>,
    windows: Vec<CaptureWindow>,
    /// 拖拽起点（全局逻辑坐标）
    origin: Option<egui::Pos2>,
    /// 当前选区（全局逻辑坐标）
    selection: Option<egui::Rect>,
    // ---- 标注 ----
    tool: Tool,
    color: usize,
    thickness: usize,
    shapes: Vec<Shape>,
    /// 正在绘制的形状起点
    draw_origin: Option<egui::Pos2>,
    /// 正在输入的文本：锚点 + 内容
    text_pos: Option<egui::Pos2>,
    text_buf: String,
    /// 8 控制点 / 整体拖动
    handle: Option<Handle>,
    move_ref: Option<(egui::Pos2, egui::Rect)>,
    /// 贴图列表（常驻，独立于覆盖层显隐）
    pins: Vec<PinImage>,
    pin_seq: u32,
}

impl Overlay {
    fn hide_all(&mut self, ctx: &egui::Context) {
        let ids: Vec<egui::ViewportId> = self.tiles.iter().map(|t| t.viewport_id()).collect();
        diag("覆盖层关闭");
        self.visible = false;
        self.origin = None;
        self.selection = None;
        self.draw_origin = None;
        self.text_pos = None;
        self.text_buf.clear();
        self.handle = None;
        self.move_ref = None;
        for id in ids {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(false));
        }
    }

    fn find_window_at(&self, pos: egui::Pos2) -> Option<egui::Rect> {
        self.windows
            .iter()
            .filter(|w| {
                let r = egui::Rect::from_min_size(
                    egui::pos2(w.x as f32, w.y as f32),
                    egui::vec2(w.width as f32, w.height as f32),
                );
                r.contains(pos)
            })
            .map(|w| {
                egui::Rect::from_min_size(
                    egui::pos2(w.x as f32, w.y as f32),
                    egui::vec2(w.width as f32, w.height as f32),
                )
            })
            // 最上层的通常是数组里靠后的窗口，取最后一个命中的
            .next_back()
    }
}

/// 截图管理器：主线程持有，负责抓屏与视口注册。
pub struct ScreenshotManager {
    overlay: Arc<Mutex<Overlay>>,
    /// 已枚举的显示器（声明覆盖层视口用）
    displays: Vec<DisplayInfo>,
    /// OCR 识别状态：None=未在识别，Some(Err/Ok)=已完成
    ocr: Arc<Mutex<Option<Result<mymac_core::ocr::OcrResult, String>>>>,
}

impl ScreenshotManager {
    pub fn new() -> Self {
        Self {
            overlay: Arc::new(Mutex::new(Overlay {
                visible: false,
                tiles: Vec::new(),
                windows: Vec::new(),
                origin: None,
                selection: None,
                tool: Tool::Select,
                color: 0,
                thickness: 1,
                shapes: Vec::new(),
                draw_origin: None,
                text_pos: None,
                text_buf: String::new(),
                handle: None,
                move_ref: None,
                pins: Vec::new(),
                pin_seq: 0,
            })),
            displays: capture::active_displays(),
            ocr: Arc::new(Mutex::new(None)),
        }
    }

    /// 触发截图（F1 / 托盘；覆盖层已存在时重新抓屏替换会话）。
    /// 抓屏（CGDisplayCreateImage）必须在主线程，logic() 即主线程。
    pub fn start(&mut self, ctx: &egui::Context) {
        diag("截图触发");
        if !capture::has_screen_capture_access() {
            diag("屏幕录制权限未授予，请求系统授权后本次跳过（授权后需重启应用）");
            capture::request_screen_capture_access();
            return;
        }
        // 重复触发时先把旧覆盖层移出屏幕，否则会把覆盖层自己抓进冻结背景
        // （多次触发后画面叠加发黑）。orderOut 同步生效，稍候再抓。
        let was_visible = crate::macos::order_out_titled_prefix("screenshot-overlay");
        if was_visible {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }

        let displays = capture::active_displays();
        if displays.is_empty() {
            diag("未枚举到任何显示器，无法截图");
            return;
        }
        self.displays = displays.clone();
        diag(format!("枚举到 {} 块显示器，开始抓屏", displays.len()));

        let windows = winlist::list_on_screen_windows();
        let mut tiles = Vec::with_capacity(displays.len());
        for (index, display) in displays.iter().enumerate() {
            match capture::grab_display_rgba(display.id) {
                Ok(grab) => {
                    if let Ok(png) = capture::encode_png(grab.rgba.clone(), grab.width, grab.height)
                    {
                        let _ = std::fs::write("/tmp/mymac-capture-debug.png", png);
                    }
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [grab.width, grab.height],
                        &grab.rgba,
                    );
                    let texture = ctx.load_texture(
                        format!("screenshot-bg-{}-{}", display.id, index),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    tiles.push(Tile {
                        display_id: display.id,
                        rect: egui::Rect::from_min_size(
                            egui::pos2(display.x as f32, display.y as f32),
                            egui::vec2(display.width as f32, display.height as f32),
                        ),
                        scale: display.scale,
                        rgba: grab.rgba,
                        px_width: grab.width,
                        px_height: grab.height,
                        texture,
                    });
                }
                Err(error) => log::warn!("抓取显示器 {id} 失败：{error}", id = display.id),
            }
        }
        if tiles.is_empty() {
            diag("抓屏结果为空，覆盖层未显示");
            return;
        }

        let id_list: Vec<egui::ViewportId> = tiles.iter().map(|t| t.viewport_id()).collect();
        diag(format!("覆盖层视口显示：{:?} ", id_list));
        {
            let mut overlay = self.overlay.lock().unwrap();
            overlay.tiles = tiles;
            overlay.windows = windows;
            overlay.origin = None;
            overlay.selection = None;
            overlay.shapes.clear();
            overlay.draw_origin = None;
            overlay.text_pos = None;
            overlay.text_buf.clear();
            overlay.handle = None;
            overlay.move_ref = None;
            overlay.tool = Tool::Select;
            overlay.visible = true;
        }
        *self.ocr.lock().unwrap() = None;
        self.declare(ctx);
        for id in id_list {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        log::info!("截图覆盖层已显示（{} 块屏幕）", self.displays.len());
    }

    /// 声明覆盖层视口。egui 要求 deferred 视口**每个 pass 都重新声明**，
    /// 否则视口会被移除——logic() 与 ui() 每次都要调用本方法。
    /// 主窗口隐藏时 ui() 不跑，logic() 仍会执行，保证覆盖层可以唤起。
    pub fn declare(&mut self, ctx: &egui::Context) {
        if self.overlay.lock().unwrap().visible {
            crate::macos::raise_windows_titled_prefix("screenshot-overlay");
        }
        {
            for display in &self.displays {
                let id = viewport_id(display.id);
                let builder = egui::ViewportBuilder::default()
                    .with_title(format!("screenshot-overlay-{}", display.id))
                    .with_visible(false)
                    .with_transparent(true)
                    .with_decorations(false)
                    .with_always_on_top()
                    .with_resizable(false)
                    .with_position(egui::pos2(display.x as f32, display.y as f32))
                    .with_inner_size([display.width as f32, display.height as f32]);
                let state = self.overlay.clone();
                let ocr = self.ocr.clone();
                let display_id = display.id;
                ctx.show_viewport_deferred(id, builder, move |ui: &mut egui::Ui, _class| {
                    overlay_viewport(display_id, &state, &ocr, ui);
                });
                diag(format!("声明覆盖层视口 display={}", display.id));
            }
        }
        // 贴图视口：独立于覆盖层常驻，同样每个 pass 重新声明（deferred 视口必须每帧重声明）
        let pins: Vec<(u32, egui::Pos2, egui::Vec2)> = {
            let overlay = self.overlay.lock().unwrap();
            overlay.pins.iter().map(|p| (p.id, p.pos, p.size)).collect()
        };
        for (id, pos, size) in pins {
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("screenshot-pin-{id}"))
                .with_visible(true)
                .with_transparent(true)
                .with_decorations(false)
                .with_always_on_top()
                .with_resizable(false)
                .with_position(pos)
                .with_inner_size([size.x, size.y]);
            let state = self.overlay.clone();
            ctx.show_viewport_deferred(
                pin_viewport_id(id),
                builder,
                move |ui: &mut egui::Ui, _class| {
                    pin_viewport(id, &state, ui);
                },
            );
        }
    }
}

/// 选区 8 个控制点（全局逻辑坐标）。
fn handle_points(sel: egui::Rect) -> [(Handle, egui::Pos2); 8] {
    let c = |x: f32, y: f32| egui::pos2(x, y);
    [
        (Handle::Nw, c(sel.min.x, sel.min.y)),
        (Handle::N, c(sel.center().x, sel.min.y)),
        (Handle::Ne, c(sel.max.x, sel.min.y)),
        (Handle::E, c(sel.max.x, sel.center().y)),
        (Handle::Se, c(sel.max.x, sel.max.y)),
        (Handle::S, c(sel.center().x, sel.max.y)),
        (Handle::Sw, c(sel.min.x, sel.max.y)),
        (Handle::W, c(sel.min.x, sel.center().y)),
    ]
}

fn handle_at(sel: egui::Rect, pos: egui::Pos2) -> Option<Handle> {
    handle_points(sel)
        .into_iter()
        .find(|(_, point)| point.distance(pos) <= 8.0)
        .map(|(handle, _)| handle)
}

/// 按控制点调整选区。
fn apply_handle(sel: egui::Rect, handle: Handle, pos: egui::Pos2) -> egui::Rect {
    let mut min = sel.min;
    let mut max = sel.max;
    match handle {
        Handle::Nw | Handle::N | Handle::W => min = pos,
        Handle::Se | Handle::S | Handle::E => max = pos,
        Handle::Ne => {
            max.x = pos.x;
            min.y = pos.y;
        }
        Handle::Sw => {
            min.x = pos.x;
            max.y = pos.y;
        }
    }
    egui::Rect::from_two_pos(min, max)
}

/// 单个覆盖层视口的每帧渲染与交互。
#[allow(clippy::too_many_lines)]
fn overlay_viewport(
    display_id: u32,
    state: &Arc<Mutex<Overlay>>,
    ocr: &Arc<Mutex<Option<Result<mymac_core::ocr::OcrResult, String>>>>,
    ui: &mut egui::Ui,
) {
    use std::collections::HashSet;
    use std::sync::OnceLock;
    static RENDER_LOGGED: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    if RENDER_LOGGED
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap()
        .insert(display_id)
    {
        diag(format!("覆盖层视口首帧渲染 display={display_id}"));
    }
    let ctx = ui.ctx().clone();
    let mut finish = false;
    let mut copy_sel: Option<egui::Rect> = None;
    let mut save_sel: Option<egui::Rect> = None;
    let mut ocr_sel: Option<egui::Rect> = None;
    let mut pin_sel: Option<egui::Rect> = None;

    #[allow(deprecated)]
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| {
            let mut overlay = state.lock().unwrap();
            if !overlay.visible {
                return;
            }
            // 先提取本显示器的静态数据，避免与后续可变借用冲突
            let Some((tile_rect, scale, texture_id)) = overlay
                .tiles
                .iter()
                .find(|t| t.display_id == display_id)
                .map(|t| (t.rect, t.scale, t.texture.id()))
            else {
                return;
            };
            let painter = ui.painter();

            // 冻结背景（uv 必须归一化到 0..1，否则越界采样导致整屏取到边缘 texel 而变色/失真）
            painter.image(
                texture_id,
                ui.max_rect(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(1.0, 1.0)),
                egui::Color32::WHITE,
            );

            // 过滤 NaN/Inf 坐标（新出现的视口在指针首次进入前可能给出非有限值）
            let pointer = ui
                .input(|i| i.pointer.interact_pos())
                .filter(|p| p.is_finite());
            // 注意：这里必须是「刚刚按下」；用 primary_down 会在按住的每一帧
            // 重复触发按下分支，把拖拽起点不断重置，框选永远出不来。
            let pressed = ui.input(|i| i.pointer.primary_pressed());
            let held = ui.input(|i| i.pointer.primary_down());
            let released = ui.input(|i| i.pointer.primary_released());

            // ---- 选区 / 标注交互（工具栏区域除外）----
            let mut toolbar_rect = egui::Rect::NOTHING;
            if let Some(sel) = overlay.selection {
                toolbar_rect = toolbar_rect_for(sel, tile_rect);
            }
            let pointer_global = pointer.map(|pos| pos + tile_rect.min.to_vec2());
            let on_toolbar = pointer_global.is_some_and(|pos| toolbar_rect.contains(pos));
            let hover = pointer_global
                .as_ref()
                .map(|pos| overlay.find_window_at(*pos).unwrap_or(tile_rect));

            if let Some(pos) = pointer_global
                && !on_toolbar
            {
                handle_selection_input(&mut overlay, pos, pressed, held, released, hover);
                handle_draw_input(&mut overlay, pos, pressed, released);
            } else if released {
                overlay.origin = None;
                overlay.draw_origin = None;
                overlay.handle = None;
                overlay.move_ref = None;
            }

            // ---- 渲染：遮罩、选区、标注 ----
            render_scene(painter, &overlay, tile_rect, scale, pointer);

            // ---- 工具栏 ----
            if let Some(sel) = overlay.selection {
                let response = draw_toolbar(ui, &mut overlay, sel, toolbar_rect, tile_rect);
                if response == ToolAction::Save {
                    save_sel = Some(sel);
                } else if response == ToolAction::Copy {
                    copy_sel = Some(sel);
                } else if response == ToolAction::Ocr {
                    ocr_sel = Some(sel);
                    *ocr.lock().unwrap() = None;
                } else if response == ToolAction::Pin {
                    pin_sel = Some(sel);
                } else if response == ToolAction::Cancel {
                    finish = true;
                }
            }

            // ---- 文本输入框 ----
            if overlay.text_pos.is_some() {
                let mut commit = false;
                let mut cancel = false;
                egui::Area::new(egui::Id::new("screenshot-text-input"))
                    .fixed_pos(overlay.text_pos.unwrap() - tile_rect.min.to_vec2())
                    .show(ui, |ui| {
                        let response = egui::TextEdit::singleline(&mut overlay.text_buf)
                            .desired_width(220.0)
                            .font(egui::TextStyle::Body)
                            .show(ui);
                        commit = response.response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        response.response.request_focus();
                    });
                if commit && !overlay.text_buf.trim().is_empty() {
                    let anchor = overlay.text_pos.unwrap();
                    let shape = Shape {
                        tool: Tool::Text,
                        rect: egui::Rect::from_min_size(anchor, egui::vec2(0.0, 0.0)),
                        text: overlay.text_buf.clone(),
                        color: COLORS[overlay.color],
                        width: THICKNESSES[overlay.thickness],
                    };
                    overlay.shapes.push(shape);
                    overlay.text_buf.clear();
                    overlay.text_pos = None;
                } else if commit || cancel {
                    overlay.text_buf.clear();
                    overlay.text_pos = None;
                }
            }

            // ---- OCR 结果面板 ----
            draw_ocr_panel(ui, ctx.clone(), ocr, tile_rect);

            // ---- 快捷键 ----
            let text_editing = overlay.text_pos.is_some();
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !text_editing {
                finish = true;
            }
            if !text_editing
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Some(sel) = overlay.selection
            {
                copy_sel = Some(sel);
                finish = true;
            }
            if finish {
                overlay.hide_all(&ctx);
            }
        });

    // ---- 锁外输出：复制 / 保存 / OCR / 贴图 ----
    if finish {
        return;
    }
    if let Some(sel) = pin_sel {
        if let Some((w, h, rgba)) = compose(state, display_id, sel) {
            let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
            let scale = state
                .lock()
                .unwrap()
                .tiles
                .iter()
                .find(|t| t.display_id == display_id)
                .map_or(1.0, |t| t.scale) as f32;
            let mut overlay = state.lock().unwrap();
            overlay.pin_seq += 1;
            let id = overlay.pin_seq;
            let texture = ctx.load_texture(
                format!("screenshot-pin-{id}"),
                image,
                egui::TextureOptions::LINEAR,
            );
            // 以 1:1 物理像素显示（逻辑尺寸 = 像素 / 缩放）
            let size = egui::vec2((w as f32 / scale).max(48.0), (h as f32 / scale).max(48.0));
            overlay.pins.push(PinImage {
                id,
                texture,
                pos: sel.min,
                size,
                drag_ref: None,
            });
            diag(format!("贴图已创建 id={id} size={size:?}"));
            // 关闭截图覆盖层，露出贴图
            overlay.hide_all(&ctx);
        }
        return;
    }
    if let Some(sel) = copy_sel {
        run_output(state, display_id, sel, |w, h, rgba| {
            let error = mymac_core::clipboard::copy_image_to_clipboard(w, h, rgba);
            if let Err(error) = error {
                log::error!("复制截图失败：{error}");
            }
        });
    }
    if let Some(sel) = save_sel {
        let composed = compose(state, display_id, sel);
        if let Some(path) = rfd::FileDialog::new()
            .set_title("保存截图")
            .set_file_name("截图.png")
            .add_filter("PNG 图片", &["png"])
            .save_file()
            && let Some((w, h, rgba)) = composed
            && let Ok(png) = capture::encode_png(rgba, w, h)
        {
            match std::fs::write(&path, png) {
                Ok(()) => log::info!("截图已保存：{}", path.display()),
                Err(error) => log::error!("保存截图失败：{error}"),
            }
        }
    }
    if let Some(sel) = ocr_sel
        && let Some((w, h, rgba)) = compose(state, display_id, sel)
    {
        let ocr = ocr.clone();
        std::thread::spawn(move || {
            let result = mymac_core::ocr::recognize_rgba(rgba, w as u32, h as u32);
            *ocr.lock().unwrap() = Some(result);
        });
    }
}

/// 选区交互：新建 / 8 控制点调整 / 整体拖动（仅「选择」工具下生效）。
fn handle_selection_input(
    overlay: &mut Overlay,
    pos: egui::Pos2,
    pressed: bool,
    held: bool,
    released: bool,
    hover: Option<egui::Rect>,
) {
    if overlay.tool != Tool::Select {
        return;
    }
    if pressed && overlay.handle.is_none() && overlay.move_ref.is_none() {
        if let Some(sel) = overlay.selection {
            if let Some(handle) = handle_at(sel, pos) {
                overlay.handle = Some(handle);
                return;
            }
            if sel.contains(pos) {
                overlay.move_ref = Some((pos, sel));
                return;
            }
        }
        overlay.origin = Some(pos);
        overlay.selection = None;
        overlay.shapes.clear();
    }
    if held
        && overlay.handle.is_some()
        && let Some(sel) = overlay.selection
        && let Some(handle) = overlay.handle
    {
        overlay.selection = Some(apply_handle(sel, handle, pos));
    } else if held && let Some((start, base)) = overlay.move_ref {
        let delta = pos - start;
        overlay.selection = Some(base.translate(delta));
    } else if held && let Some(origin) = overlay.origin {
        let rect = egui::Rect::from_two_pos(origin, pos);
        overlay.selection = Some(rect);
        if released {
            if rect.width() < 4.0 || rect.height() < 4.0 {
                // 几乎没有拖动 → 视为单击，选中悬停目标（窗口或整屏）
                overlay.selection = hover;
            }
            overlay.origin = None;
        }
    }
    if released {
        overlay.handle = None;
        overlay.move_ref = None;
    }
}

/// 标注绘制输入（矩形 / 箭头；文本点击落锚点）。
fn handle_draw_input(overlay: &mut Overlay, pos: egui::Pos2, pressed: bool, released: bool) {
    if overlay.tool == Tool::Select || !overlay.selection.is_some_and(|s| s.contains(pos)) {
        return;
    }
    if overlay.tool == Tool::Text {
        if pressed {
            // 提交未完成的上一段文本，开始新锚点
            overlay.text_pos = Some(pos);
            overlay.text_buf.clear();
        }
        return;
    }
    if pressed && overlay.draw_origin.is_none() {
        overlay.draw_origin = Some(pos);
    }
    if let Some(origin) = overlay.draw_origin {
        let rect = egui::Rect::from_two_pos(origin, pos);
        if released {
            if rect.width() > 2.0 || rect.height() > 2.0 {
                overlay.shapes.push(Shape {
                    tool: overlay.tool,
                    rect,
                    text: String::new(),
                    color: COLORS[overlay.color],
                    width: THICKNESSES[overlay.thickness],
                });
            }
            overlay.draw_origin = None;
        }
    }
}

/// 场景渲染：遮罩、选区边框与控制点、已提交与进行中的标注、悬停高亮。
fn render_scene(
    painter: &egui::Painter,
    overlay: &Overlay,
    tile_rect: egui::Rect,
    scale: f64,
    pointer: Option<egui::Pos2>,
) {
    let to_local = |r: egui::Rect| r.intersect(tile_rect).translate(-tile_rect.min.to_vec2());

    // 标注（全局坐标，裁剪到选区）
    let clip = overlay.selection.map(to_local);
    let shape_painter = match clip {
        Some(clip) if clip.is_positive() => painter.with_clip_rect(clip),
        _ => painter.clone(),
    };
    for shape in &overlay.shapes {
        draw_shape_shape(&shape_painter, shape, -tile_rect.min.to_vec2());
    }
    if let (Some(origin), Some(_), Some(pos)) = (overlay.draw_origin, overlay.selection, pointer) {
        let preview = Shape {
            tool: overlay.tool,
            rect: egui::Rect::from_two_pos(origin, pos),
            text: String::new(),
            color: COLORS[overlay.color],
            width: THICKNESSES[overlay.thickness],
        };
        draw_shape_shape(&shape_painter, &preview, -tile_rect.min.to_vec2());
    }
    if let Some(anchor) = overlay.text_pos {
        shape_painter.rect_filled(
            egui::Rect::from_min_size(anchor - tile_rect.min.to_vec2(), egui::vec2(4.0, 18.0)),
            0,
            egui::Color32::WHITE,
        );
    }

    // 选区边框 + 控制点 + 尺寸提示
    if let Some(sel) = overlay.selection {
        let local = to_local(sel);
        if local.is_positive() {
            painter.rect_stroke(
                local,
                0.0,
                egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
                egui::StrokeKind::Outside,
            );
            for (_, point) in handle_points(sel) {
                painter.rect_filled(
                    egui::Rect::from_center_size(
                        point - tile_rect.min.to_vec2(),
                        egui::vec2(8.0, 8.0),
                    ),
                    2,
                    egui::Color32::WHITE,
                );
            }
            let pw = (sel.width() * scale as f32).round() as u32;
            let ph = (sel.height() * scale as f32).round() as u32;
            painter.text(
                local.left_top() + egui::vec2(0.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{pw} × {ph}"),
                egui::FontId::proportional(12.0),
                egui::Color32::WHITE,
            );
        }
    } else if let Some(pos) = pointer {
        // 无选区时高亮悬停目标
        let global = pos + tile_rect.min.to_vec2();
        if let Some(hover) = overlay.find_window_at(global).or(Some(tile_rect)) {
            let local = to_local(hover);
            if local.is_positive() {
                painter.rect_filled(local, 0.0, egui::Color32::from_white_alpha(28));
                painter.rect_stroke(
                    local,
                    0.0,
                    egui::Stroke::new(2.0_f32, egui::Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}

/// 绘制单个标注形状（egui 画笔，用于实时预览与已提交形状）。
fn draw_shape_shape(painter: &egui::Painter, shape: &Shape, offset: egui::Vec2) {
    let color = egui::Color32::from_rgba_unmultiplied(
        shape.color[0],
        shape.color[1],
        shape.color[2],
        shape.color[3],
    );
    let stroke = egui::Stroke::new(shape.width, color);
    match shape.tool {
        Tool::Rect => {
            painter.rect_stroke(
                shape.rect.translate(offset),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Tool::Arrow => {
            let start = shape.rect.min + offset;
            let end = shape.rect.max + offset;
            painter.line_segment([start, end], stroke);
            // 箭头头部：两条短边
            let dir = (end - start).normalized();
            let len = shape.width * 4.0 + 6.0;
            let left = egui::pos2(
                end.x - (dir.x * 0.5 - dir.y * 0.866) * len,
                end.y - (dir.y * 0.5 + dir.x * 0.866) * len,
            );
            let right = egui::pos2(
                end.x - (dir.x * 0.5 + dir.y * 0.866) * len,
                end.y - (dir.y * 0.5 - dir.x * 0.866) * len,
            );
            painter.line_segment([end, left], stroke);
            painter.line_segment([end, right], stroke);
        }
        Tool::Text => {
            painter.text(
                shape.rect.min + offset,
                egui::Align2::LEFT_TOP,
                &shape.text,
                egui::FontId::proportional(14.0 + shape.width * 2.0),
                color,
            );
        }
        Tool::Select => {}
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolAction {
    None,
    Copy,
    Save,
    Ocr,
    Pin,
    Cancel,
}

/// 工具栏位置：选区上方，贴顶时放选区下方。
fn toolbar_rect_for(sel: egui::Rect, tile_rect: egui::Rect) -> egui::Rect {
    const H: f32 = 36.0;
    const W: f32 = 420.0;
    if !sel.is_finite() || !tile_rect.is_finite() {
        diag(format!(
            "工具栏矩形含非有限值：sel={sel:?} tile={tile_rect:?}"
        ));
        return egui::Rect::NOTHING;
    }
    let x = (sel.center().x - W / 2.0).clamp(tile_rect.min.x, tile_rect.max.x - W);
    let y = if sel.min.y - H - 8.0 >= tile_rect.min.y {
        sel.min.y - H - 8.0
    } else {
        (sel.max.y + 8.0).clamp(tile_rect.min.y, tile_rect.max.y - H)
    };
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(W, H))
}

/// 渲染工具栏，返回触发的动作。
fn draw_toolbar(
    ui: &mut egui::Ui,
    overlay: &mut Overlay,
    sel: egui::Rect,
    toolbar_rect: egui::Rect,
    tile_rect: egui::Rect,
) -> ToolAction {
    use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
    use shadcn_egui::icons::lucide;

    let mut action = ToolAction::None;
    if !toolbar_rect.is_positive() {
        return action;
    }
    let area = egui::Area::new(egui::Id::new("screenshot-toolbar"))
        .fixed_pos(toolbar_rect.min - tile_rect.min.to_vec2());
    let theme = shadcn_egui::Theme::current(ui.ctx());
    let palette = theme.palette;
    area.show(ui, |ui| {
        egui::Frame::new()
            .fill(palette.card)
            .stroke(egui::Stroke::new(1.0_f32, palette.border))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut tool_button = |ui: &mut egui::Ui, tool: Tool, icon, tip: &str| {
                        let active = overlay.tool == tool;
                        let button = Button::new("")
                            .variant(if active {
                                ButtonVariant::Secondary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .size(ButtonSize::IconSm)
                            .icon(icon)
                            .enabled(true);
                        if ui.add(button).on_hover_text(tip).clicked() {
                            overlay.tool = tool;
                            overlay.text_pos = None;
                            overlay.text_buf.clear();
                        }
                    };
                    tool_button(ui, Tool::Select, &lucide::MOVE, "选择 / 调整选区");
                    tool_button(ui, Tool::Rect, &lucide::SQUARE, "矩形标注");
                    tool_button(ui, Tool::Arrow, &lucide::ARROW_UP_RIGHT, "箭头标注");
                    tool_button(ui, Tool::Text, &lucide::TYPE, "文本标注");
                    ui.separator();
                    // 颜色板
                    for (index, color) in COLORS.iter().enumerate() {
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                        let swatch = egui::Color32::from_rgba_unmultiplied(
                            color[0], color[1], color[2], 255,
                        );
                        if overlay.color == index {
                            ui.painter().rect_filled(rect, 4.0, swatch);
                            ui.painter().rect_stroke(
                                rect,
                                4.0,
                                egui::Stroke::new(2.0_f32, palette.primary),
                                egui::StrokeKind::Inside,
                            );
                        } else {
                            ui.painter().rect_filled(rect, 4.0, swatch);
                        }
                        if response.clicked() {
                            overlay.color = index;
                        }
                    }
                    ui.separator();
                    for (index, label) in ["细", "中", "粗"].iter().enumerate() {
                        let active = overlay.thickness == index;
                        let button = Button::new(*label)
                            .variant(if active {
                                ButtonVariant::Secondary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .size(ButtonSize::Sm);
                        if ui.add(button).clicked() {
                            overlay.thickness = index;
                        }
                    }
                    ui.separator();
                    if ui
                        .add(
                            Button::new("")
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::IconSm)
                                .icon(&lucide::UNDO),
                        )
                        .on_hover_text("撤销标注")
                        .clicked()
                    {
                        overlay.shapes.pop();
                    }
                    if ui
                        .add(
                            Button::new("识别")
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Sm)
                                .icon(&lucide::SCAN_TEXT),
                        )
                        .clicked()
                    {
                        action = ToolAction::Ocr;
                    }
                    if ui
                        .add(
                            Button::new("保存")
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Sm)
                                .icon(&lucide::SAVE),
                        )
                        .clicked()
                    {
                        action = ToolAction::Save;
                    }
                    if ui
                        .add(Button::new("复制").size(ButtonSize::Sm).icon(&lucide::COPY))
                        .clicked()
                    {
                        action = ToolAction::Copy;
                    }
                    if ui
                        .add(
                            Button::new("贴图")
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Sm)
                                .icon(&lucide::PIN),
                        )
                        .on_hover_text("贴图：置顶显示，可拖动 / 滚轮缩放 / 右键关闭")
                        .clicked()
                    {
                        action = ToolAction::Pin;
                    }
                    if ui
                        .add(
                            Button::new("")
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::IconSm)
                                .icon(&lucide::X),
                        )
                        .on_hover_text("关闭（Esc）")
                        .clicked()
                    {
                        action = ToolAction::Cancel;
                    }
                    // 标记选区锚点，便于引用参数
                    let _ = sel;
                });
            });
    });
    action
}

/// OCR 结果面板（固定在视口右下角）。
fn draw_ocr_panel(
    ui: &mut egui::Ui,
    ctx: egui::Context,
    ocr: &Arc<Mutex<Option<Result<mymac_core::ocr::OcrResult, String>>>>,
    tile_rect: egui::Rect,
) {
    let guard = ocr.lock().unwrap();
    let Some(result) = guard.as_ref() else {
        if ocr_strong_count(ocr) > 0 {
            // 识别中：给个轻提示
        }
        return;
    };
    let lines: Vec<String> = match result {
        Ok(result) => result.lines.iter().map(|l| l.text.clone()).collect(),
        Err(error) => vec![format!("识别失败：{error}")],
    };
    drop(guard);

    let mut copy_all = false;
    let mut close = false;
    let theme = shadcn_egui::Theme::current(&ctx);
    let palette = theme.palette;
    egui::Area::new(egui::Id::new("screenshot-ocr-panel"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-24.0, -24.0))
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(palette.card)
                .stroke(egui::Stroke::new(1.0_f32, palette.border))
                .corner_radius(8)
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("识别结果").strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    Button::new("")
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::IconSm)
                                        .icon(&shadcn_egui::icons::lucide::X),
                                )
                                .on_hover_text("关闭")
                                .clicked()
                            {
                                close = true;
                            }
                            if ui
                                .add(
                                    Button::new("复制全部")
                                        .variant(ButtonVariant::Outline)
                                        .size(ButtonSize::Sm),
                                )
                                .clicked()
                            {
                                copy_all = true;
                            }
                        });
                    });
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .min_scrolled_width(320.0)
                        .show(ui, |ui| {
                            for line in &lines {
                                ui.label(RichText::new(line).size(13.0));
                            }
                        });
                    let _ = tile_rect;
                });
        });

    if copy_all {
        let text = lines.join("\n");
        std::thread::spawn(move || {
            if let Err(error) = arboard_copy_text(text) {
                log::error!("复制识别文字失败：{error}");
            }
        });
    }
    if close {
        *ocr.lock().unwrap() = None;
    }
}

fn ocr_strong_count(
    _ocr: &Arc<Mutex<Option<Result<mymac_core::ocr::OcrResult, String>>>>,
) -> usize {
    0
}

fn arboard_copy_text(text: String) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())
}

/// 裁剪选区并叠加标注，返回合成后的 (宽, 高, RGBA)。
fn compose(
    state: &Arc<Mutex<Overlay>>,
    display_id: u32,
    sel: egui::Rect,
) -> Option<(usize, usize, Vec<u8>)> {
    let overlay = state.lock().unwrap();
    let tile = overlay.tiles.iter().find(|t| t.display_id == display_id)?;
    let crop = crop_selection(tile, sel)?;
    let scale = tile.scale as f32;

    // 标注栅格化：叠加到选区像素上
    let raster = overlay_text_raster();
    let px_w = crop.0;
    let px_h = crop.1;
    let shapes: Vec<Shape> = overlay.shapes.clone();
    let mut rgba = crop.2;
    for shape in &shapes {
        let local = shape.rect.intersect(sel);
        if !local.is_positive() && shape.tool != Tool::Text {
            continue;
        }
        let color = shape.color;
        let thickness = ((shape.width * scale).round() as i32).max(1);
        let to_px = |p: egui::Pos2| {
            (
                ((p.x - sel.min.x) * scale).round() as i32,
                ((p.y - sel.min.y) * scale).round() as i32,
            )
        };
        match shape.tool {
            Tool::Rect => {
                let (x0, y0) = to_px(local.min);
                let (x1, y1) = to_px(local.max);
                draw_line(&mut rgba, px_w, px_h, x0, y0, x1, y0, color, thickness);
                draw_line(&mut rgba, px_w, px_h, x1, y0, x1, y1, color, thickness);
                draw_line(&mut rgba, px_w, px_h, x1, y1, x0, y1, color, thickness);
                draw_line(&mut rgba, px_w, px_h, x0, y1, x0, y0, color, thickness);
            }
            Tool::Arrow => {
                let (x0, y0) = to_px(shape.rect.min);
                let (x1, y1) = to_px(shape.rect.max);
                draw_line(&mut rgba, px_w, px_h, x0, y0, x1, y1, color, thickness);
                let (dx, dy) = (x1 - x0, y1 - y0);
                let length = ((dx * dx + dy * dy) as f32).sqrt().max(1.0);
                let (ux, uy) = (dx as f32 / length, dy as f32 / length);
                let head = (thickness * 4) as f32;
                for sign in [-1.0_f32, 1.0] {
                    let hx = x1 as f32 - (ux * 0.5 - uy * sign * 0.866) * head;
                    let hy = y1 as f32 - (uy * 0.5 + ux * sign * 0.866) * head;
                    draw_line(
                        &mut rgba,
                        px_w,
                        px_h,
                        x1,
                        y1,
                        hx.round() as i32,
                        hy.round() as i32,
                        color,
                        thickness,
                    );
                }
            }
            Tool::Text => {
                let raster_guard = raster.lock().unwrap();
                if let Some(raster) = raster_guard.as_ref()
                    && !shape.text.is_empty()
                {
                    let (x, y) = to_px(shape.rect.min);
                    raster.draw(
                        &mut rgba,
                        px_w,
                        px_h,
                        x + 2,
                        y + 2,
                        &shape.text,
                        (14.0 + shape.width * 2.0) * scale,
                        color,
                    );
                }
            }
            Tool::Select => {}
        }
    }
    Some((px_w, px_h, rgba))
}

/// 共享的文字栅格化器（懒加载，加载失败时文本标注只保留 egui 预览）。
fn overlay_text_raster() -> Arc<Mutex<Option<TextRaster>>> {
    use std::sync::OnceLock;
    static RASTER: OnceLock<Arc<Mutex<Option<TextRaster>>>> = OnceLock::new();
    RASTER
        .get_or_init(|| Arc::new(Mutex::new(TextRaster::new())))
        .clone()
}

/// 粗线段：沿线段等距填充圆点。
#[allow(clippy::too_many_arguments)]
fn draw_line(
    rgba: &mut [u8],
    px_w: usize,
    px_h: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: [u8; 4],
    thickness: i32,
) {
    let radius = (thickness / 2).max(1);
    let dx = (x1 - x0) as f32;
    let dy = (y1 - y0) as f32;
    let length = (dx * dx + dy * dy).sqrt().max(1.0);
    let steps = length.ceil() as i32 * 2;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let cx = x0 + (dx * t).round() as i32;
        let cy = y0 + (dy * t).round() as i32;
        for dy_off in -radius..=radius {
            for dx_off in -radius..=radius {
                if dx_off * dx_off + dy_off * dy_off > radius * radius {
                    continue;
                }
                let px = cx + dx_off;
                let py = cy + dy_off;
                if px < 0 || py < 0 || px as usize >= px_w || py as usize >= px_h {
                    continue;
                }
                let base = (py as usize * px_w + px as usize) * 4;
                rgba[base] = color[0];
                rgba[base + 1] = color[1];
                rgba[base + 2] = color[2];
                rgba[base + 3] = 255;
            }
        }
    }
}

/// 从冻结背景裁剪选区，返回 (宽, 高, RGBA)。
fn crop_selection(tile: &Tile, sel: egui::Rect) -> Option<(usize, usize, Vec<u8>)> {
    let local = sel.intersect(tile.rect);
    if !local.is_positive() {
        return None;
    }
    let scale = tile.scale as f32;
    let x0 =
        (((local.min.x - tile.rect.min.x) * scale).floor() as usize).clamp(0, tile.px_width - 1);
    let y0 =
        (((local.min.y - tile.rect.min.y) * scale).floor() as usize).clamp(0, tile.px_height - 1);
    let x1 =
        (((local.max.x - tile.rect.min.x) * scale).ceil() as usize).clamp(x0 + 1, tile.px_width);
    let y1 =
        (((local.max.y - tile.rect.min.y) * scale).ceil() as usize).clamp(y0 + 1, tile.px_height);
    let width = x1 - x0;
    let height = y1 - y0;
    let mut out = Vec::with_capacity(width * height * 4);
    for row in y0..y1 {
        let start = (row * tile.px_width + x0) * 4;
        let end = start + width * 4;
        out.extend_from_slice(&tile.rgba[start..end]);
    }
    Some((width, height, out))
}

/// 合成输出：复制与保存共用。
fn run_output(
    state: &Arc<Mutex<Overlay>>,
    display_id: u32,
    sel: egui::Rect,
    sink: impl FnOnce(usize, usize, Vec<u8>),
) {
    if let Some((w, h, rgba)) = compose(state, display_id, sel) {
        sink(w, h, rgba);
    }
}

/// 贴图视口每帧渲染与交互：拖动移动、滚轮缩放、右键关闭。
fn pin_viewport(pin_id: u32, state: &Arc<Mutex<Overlay>>, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    // 快照本贴图的纹理与位置（贴图可能已被右键删除）
    let snapshot = {
        let overlay = state.lock().unwrap();
        overlay
            .pins
            .iter()
            .find(|p| p.id == pin_id)
            .map(|p| (p.texture.id(), p.pos))
    };
    let Some((texture_id, pin_pos)) = snapshot else {
        // 贴图已移除：关闭对应视口
        ctx.send_viewport_cmd_to(pin_viewport_id(pin_id), egui::ViewportCommand::Close);
        return;
    };

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| {
            let painter = ui.painter();
            let rect = ui.max_rect();
            painter.image(
                texture_id,
                rect,
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            // 细白边，便于在浅色背景上分辨贴图边界
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0_f32, egui::Color32::WHITE),
                egui::StrokeKind::Outside,
            );

            // 指针坐标转全局屏幕坐标：窗口跟随光标移动时局部坐标会漂移，
            // 用「窗口位置 + 局部坐标」得到稳定全局坐标，拖动才不抖动 / 跑飞。
            let pointer_local = ui
                .input(|i| i.pointer.interact_pos())
                .filter(|p| p.is_finite());
            let pointer_global = pointer_local.map(|p| p + pin_pos.to_vec2());
            let pressed = ui.input(|i| i.pointer.primary_pressed());
            let down = ui.input(|i| i.pointer.primary_down());
            let released = ui.input(|i| i.pointer.primary_released());
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            let right = ui.input(|i| i.pointer.secondary_clicked());

            // 拖动移动（按全局坐标增量，避免窗口跟随光标时的反馈漂移）
            if pressed {
                let mut overlay = state.lock().unwrap();
                if let Some(pin) = overlay.pins.iter_mut().find(|p| p.id == pin_id) {
                    pin.drag_ref = pointer_global;
                }
            } else if down {
                let mut overlay = state.lock().unwrap();
                if let Some(pin) = overlay.pins.iter_mut().find(|p| p.id == pin_id)
                    && let (Some(start), Some(cur)) = (pin.drag_ref, pointer_global)
                {
                    pin.pos += cur - start;
                    pin.drag_ref = Some(cur);
                }
            } else if released {
                let mut overlay = state.lock().unwrap();
                if let Some(pin) = overlay.pins.iter_mut().find(|p| p.id == pin_id) {
                    pin.drag_ref = None;
                }
            }

            // 滚轮缩放（保持中心不动）
            if scroll.abs() > 0.5 {
                let factor = 1.0 + (scroll * 0.002).clamp(-0.15, 0.15);
                let mut overlay = state.lock().unwrap();
                if let Some(pin) = overlay.pins.iter_mut().find(|p| p.id == pin_id) {
                    let center = pin.pos + pin.size * 0.5;
                    let size = egui::vec2(
                        (pin.size.x * factor).clamp(48.0, 6000.0),
                        (pin.size.y * factor).clamp(48.0, 6000.0),
                    );
                    pin.size = size;
                    pin.pos = center - size * 0.5;
                }
            }

            // 右键关闭
            if right {
                let mut overlay = state.lock().unwrap();
                overlay.pins.retain(|p| p.id != pin_id);
                ctx.send_viewport_cmd_to(pin_viewport_id(pin_id), egui::ViewportCommand::Close);
            }
        });
}
