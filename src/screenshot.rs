//! 截图覆盖层：每显示器一个全屏置顶透明视口 + 冻结背景纹理。
//!
//! 交互（M5）：
//! - 悬停高亮应用窗口；悬停无窗口区域高亮整屏（M5 新需求 1）
//! - 单击选中（窗口 / 整屏），拖拽自由框选，8 控制点调整大小，整体拖动
//! - 标注：矩形、箭头、文本（fontdue 离线栅格化），颜色 / 粗细 / 撤销
//! - 输出：Enter / 复制按钮合成选区写剪贴板；保存文件（rfd）；识别文字（Vision OCR）
//! - 覆盖层存在时再触发（F1 / 托盘）→ 重新抓屏替换会话（M5 新需求 2）
//!
//! 贴图（M6）：置顶小窗，拖动移动、滚轮缩放、双击 / Esc / 关闭按钮关闭、右键菜单；独立于覆盖层常驻。

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;

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
const COLORS: [[u8; 4]; 6] = [
    [229, 72, 77, 255],
    [245, 165, 36, 255],
    [48, 164, 108, 255],
    [59, 130, 246, 255],
    [255, 255, 255, 255],
    [17, 17, 17, 255],
];
/// 贴图初始显示尺寸上限（逻辑像素，超出时等比缩小）
const PIN_MAX_SIZE: egui::Vec2 = egui::vec2(1100.0, 760.0);
/// 贴图缩放倍率范围（相对初始尺寸）
const PIN_ZOOM_RANGE: (f32, f32) = (0.12, 6.0);
/// 提示 toast 显示时长（秒）
const TOAST_SECS: f64 = 2.2;
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
    /// 冻结背景纹理。必须在覆盖层视口自己的 pass 里创建：eframe 只在视口可见时上传
    /// 该 pass 的纹理增量，若在主窗口（隐藏 / 被遮挡）的 logic 里 load_texture，
    /// 增量会被丢弃，GPU 上缺纹理，覆盖层只剩遮罩而整屏发黑。
    pub texture: Option<egui::TextureHandle>,
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
    /// 原始像素（右键「复制到粘贴板」用）
    rgba: Arc<Vec<u8>>,
    px_size: [usize; 2],
    /// 窗口位置（全局逻辑坐标）
    pos: egui::Pos2,
    /// 初始显示尺寸（已限制在 PIN_MAX_SIZE 内并保持比例）
    base: egui::Vec2,
    /// 缩放倍率（PIN_ZOOM_RANGE 内）
    zoom: f32,
    /// 拖动起点（全局逻辑坐标）
    drag_ref: Option<egui::Pos2>,
}

impl PinImage {
    fn size(&self) -> egui::Vec2 {
        self.base * self.zoom
    }
}

/// 贴图初始尺寸：物理像素按缩放换算为逻辑尺寸，超出上限时等比缩小。
fn pin_base_size(px_w: usize, px_h: usize, scale: f32) -> egui::Vec2 {
    let logical = egui::vec2(px_w as f32 / scale, px_h as f32 / scale);
    let fit = (PIN_MAX_SIZE.x / logical.x)
        .min(PIN_MAX_SIZE.y / logical.y)
        .min(1.0);
    logical * fit
}

/// 标注文字的离线栅格化（共享一次字体加载）。
/// 优先中文字体（PingFang / 冬青黑体 / 华文黑体），逐字回退到 SFNS 等西文字体。
struct TextRaster {
    fonts: Vec<fontdue::Font>,
}

impl TextRaster {
    fn new() -> Option<Self> {
        const CANDIDATES: [&str; 6] = [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ];
        let fonts: Vec<fontdue::Font> = CANDIDATES
            .iter()
            .filter_map(|path| std::fs::read(path).ok())
            .filter_map(|bytes| {
                // ttc 是字体集合：取第 0 个字体
                let settings = fontdue::FontSettings {
                    collection_index: 0,
                    ..fontdue::FontSettings::default()
                };
                fontdue::Font::from_bytes(bytes, settings).ok()
            })
            .collect();
        if fonts.is_empty() {
            diag("未找到可用于标注栅格化的系统字体");
            return None;
        }
        Some(Self { fonts })
    }

    /// 选出包含该字符字形的字体（都没有时用第一个）。
    fn font_for(&self, ch: char) -> &fontdue::Font {
        self.fonts
            .iter()
            .find(|f| f.lookup_glyph_index(ch) != 0)
            .unwrap_or(&self.fonts[0])
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
        // y 为文字左上角；fontdue 的 ymin 相对基线，需先换算基线位置
        let ascent = self.fonts[0]
            .horizontal_line_metrics(size)
            .map_or(size * 0.8, |m| m.ascent)
            .round() as i32;
        let baseline = y + ascent;
        let mut pen = x;
        for ch in text.chars() {
            let font = self.font_for(ch);
            let (metrics, bitmap) = font.rasterize(ch, size);
            let top = baseline - (metrics.height as i32 + metrics.ymin);
            for (index, coverage) in bitmap.iter().enumerate() {
                if *coverage == 0 {
                    continue;
                }
                let gx = pen + metrics.xmin + (index % metrics.width) as i32;
                let gy = top + (index / metrics.width) as i32;
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

/// OCR 状态：Idle 未识别 / Loading 识别中 / Done 已出结果（逐行可多选）。
enum OcrState {
    Idle,
    Loading,
    Done {
        lines: Vec<mymac_core::ocr::OcrLine>,
        selected: BTreeSet<usize>,
        hover: Option<usize>,
    },
}

/// 覆盖层共享状态（主线程与各视口回调、OCR 线程之间共享）。
struct Overlay {
    visible: bool,
    tiles: Vec<Tile>,
    windows: Vec<CaptureWindow>,
    /// 拖拽起点（全局逻辑坐标）
    origin: Option<egui::Pos2>,
    /// 当前选区（全局逻辑坐标，限制在单块屏幕内）
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
    /// 会话序号：每次开始 / 结束截图递增，丢弃过期的 OCR 结果
    session: u64,
    ocr: OcrState,
    /// 提示 toast：文案 + 出现时刻
    toast: Option<(String, Instant)>,
    /// 截图前主窗口是否可见（结束后恢复；否则交还焦点给之前的应用）
    restore_main: bool,
}

impl Overlay {
    fn show_toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }

    /// 提交正在输入的文本标注（空文本直接丢弃）。
    fn commit_text(&mut self) {
        let Some(anchor) = self.text_pos.take() else {
            return;
        };
        let text = std::mem::take(&mut self.text_buf);
        if text.trim().is_empty() {
            return;
        }
        self.shapes.push(Shape {
            tool: Tool::Text,
            rect: egui::Rect::from_min_size(anchor, egui::Vec2::ZERO),
            text,
            color: COLORS[self.color],
            width: THICKNESSES[self.thickness],
        });
    }

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
        self.ocr = OcrState::Idle;
        self.toast = None;
        self.session += 1;
        for id in ids {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(false));
        }
        if std::mem::take(&mut self.restore_main) {
            crate::macos::order_main_window(true);
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        } else {
            crate::macos::deactivate_app();
        }
    }

    fn find_window_at(&self, pos: egui::Pos2) -> Option<egui::Rect> {
        self.windows
            .iter()
            .map(|w| {
                egui::Rect::from_min_size(
                    egui::pos2(w.x as f32, w.y as f32),
                    egui::vec2(w.width as f32, w.height as f32),
                )
            })
            .filter(|r| r.contains(pos))
            // 与旧版 / 需求一致：包含光标的最小面积窗口优先（便于选中被大窗口包住的小窗口）
            .min_by(|a, b| a.area().total_cmp(&b.area()))
    }
}

/// 截图管理器：主线程持有，负责抓屏与视口注册。
pub struct ScreenshotManager {
    overlay: Arc<Mutex<Overlay>>,
    /// 已枚举的显示器（声明覆盖层视口用）
    displays: Vec<DisplayInfo>,
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
                session: 0,
                ocr: OcrState::Idle,
                toast: None,
                restore_main: false,
            })),
            displays: capture::active_displays(),
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
        // 抓屏前把旧覆盖层与主窗口移出屏幕，否则会被截进冻结背景。
        // orderOut 同步生效，窗口确实在屏幕上时稍候再抓（等合成器刷新）。
        let overlay_was_visible = crate::macos::order_out_titled_prefix("screenshot-overlay");
        let main_visible = crate::macos::main_window_visible();
        if main_visible {
            crate::macos::order_main_window(false);
        }
        if overlay_was_visible || main_visible {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }

        let displays = capture::active_displays();
        if displays.is_empty() {
            diag("未枚举到任何显示器，无法截图");
            if main_visible {
                crate::macos::order_main_window(true);
            }
            return;
        }
        self.displays = displays.clone();
        diag(format!("枚举到 {} 块显示器，开始抓屏", displays.len()));

        let windows = winlist::list_on_screen_windows();
        let mut tiles = Vec::with_capacity(displays.len());
        for display in &displays {
            match capture::grab_display_rgba(display.id) {
                Ok(grab) => {
                    diag(format!(
                        "抓屏 display={} {}x{} 像素统计：{}",
                        display.id,
                        grab.width,
                        grab.height,
                        pixel_stats(&grab.rgba)
                    ));
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
                        texture: None,
                    });
                }
                Err(error) => diag(format!("抓取显示器 {} 失败：{error}", display.id)),
            }
        }
        if tiles.is_empty() {
            diag("抓屏结果为空，覆盖层未显示");
            if main_visible {
                crate::macos::order_main_window(true);
            }
            return;
        }

        let id_list: Vec<egui::ViewportId> = tiles.iter().map(|t| t.viewport_id()).collect();
        {
            let mut overlay = self.overlay.lock().unwrap();
            // 重复触发时主窗口已被上一次会话移走，沿用上一次的恢复标记
            overlay.restore_main = if overlay.visible {
                overlay.restore_main || main_visible
            } else {
                main_visible
            };
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
            overlay.ocr = OcrState::Idle;
            overlay.toast = None;
            overlay.session += 1;
            overlay.visible = true;
        }
        // 托盘常驻（Accessory）时必须激活应用，覆盖层才能拿到键盘焦点
        crate::macos::activate_app();
        self.declare(ctx);
        for id in id_list {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        diag(format!(
            "截图覆盖层已显示（{} 块屏幕）",
            self.displays.len()
        ));
    }

    /// 声明覆盖层视口。egui 要求 deferred 视口**每个 pass 都重新声明**，
    /// 否则视口会被移除——logic() 与 ui() 每次都要调用本方法。
    /// 主窗口隐藏时 ui() 不跑，logic() 仍会执行，保证覆盖层可以唤起。
    pub fn declare(&mut self, ctx: &egui::Context) {
        if self.overlay.lock().unwrap().visible {
            crate::macos::raise_windows_titled_prefix("screenshot-overlay");
        }
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
            let display_id = display.id;
            ctx.show_viewport_deferred(id, builder, move |ui: &mut egui::Ui, _class| {
                overlay_viewport(display_id, &state, ui);
            });
        }
        // 贴图视口：独立于覆盖层常驻，同样每个 pass 重新声明（deferred 视口必须每帧重声明）
        let pins: Vec<(u32, egui::Pos2, egui::Vec2)> = {
            let overlay = self.overlay.lock().unwrap();
            overlay
                .pins
                .iter()
                .map(|p| (p.id, p.pos, p.size()))
                .collect()
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

/// 抓屏结果的抽样统计（平均亮度、平均 alpha、纯黑占比），用于诊断黑屏。
fn pixel_stats(rgba: &[u8]) -> String {
    let pixels = rgba.as_chunks::<4>().0;
    let step = (pixels.len() / 4096).max(1);
    let (mut n, mut luma, mut alpha, mut black) = (0u64, 0u64, 0u64, 0u64);
    for p in pixels.iter().step_by(step) {
        n += 1;
        let l = (u64::from(p[0]) * 3 + u64::from(p[1]) * 6 + u64::from(p[2])) / 10;
        luma += l;
        alpha += u64::from(p[3]);
        if l < 8 {
            black += 1;
        }
    }
    let n = n.max(1);
    format!(
        "luma={} alpha={} black={}%",
        luma / n,
        alpha / n,
        black * 100 / n
    )
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
        Handle::Nw => min = pos,
        Handle::N => min.y = pos.y,
        Handle::W => min.x = pos.x,
        Handle::Se => max = pos,
        Handle::S => max.y = pos.y,
        Handle::E => max.x = pos.x,
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

/// 标注文字字号（逻辑像素），预览与栅格化共用。
fn text_size(width: f32) -> f32 {
    14.0 + width * 2.0
}

fn color32(color: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3])
}

/// 单个覆盖层视口的每帧渲染与交互。
#[allow(clippy::too_many_lines)]
fn overlay_viewport(display_id: u32, state: &Arc<Mutex<Overlay>>, ui: &mut egui::Ui) {
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
    // 锁内产生、锁外执行的输出动作（合成需要重新加锁）
    let mut action = ToolAction::None;
    let mut action_sel: Option<egui::Rect> = None;

    #[allow(deprecated)]
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| {
            let mut overlay = state.lock().unwrap();
            if !overlay.visible {
                return;
            }
            // 先提取本显示器的静态数据，避免与后续可变借用冲突
            let Some(tile) = overlay
                .tiles
                .iter_mut()
                .find(|t| t.display_id == display_id)
            else {
                return;
            };
            let texture_id = tile
                .texture
                .get_or_insert_with(|| {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [tile.px_width, tile.px_height],
                        &tile.rgba,
                    );
                    ctx.load_texture(
                        format!("screenshot-bg-{display_id}"),
                        image,
                        egui::TextureOptions::LINEAR,
                    )
                })
                .id();
            let (tile_rect, scale) = (tile.rect, tile.scale);
            let painter = ui.painter().clone();

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

            // ---- 选区 / 标注交互 ----
            // 指针在工具栏、文本框、OCR 面板等浮层上时不处理画布输入；
            // 但已经开始的拖拽（框选、调整、绘制）要继续跟随，不能中途停住。
            // 浮层（工具栏、文本框、OCR 面板、菜单）都是 Foreground 及以上层级，
            // 画布本身是 Background；按指针下的图层判定，不依赖 root_ui_available_rect。
            let over_ui = pointer.is_some_and(|p| {
                ctx.layer_id_at(p)
                    .is_some_and(|layer| layer.order != egui::Order::Background)
            });
            let busy = overlay.origin.is_some()
                || overlay.handle.is_some()
                || overlay.move_ref.is_some()
                || overlay.draw_origin.is_some();
            let ocr_open = matches!(overlay.ocr, OcrState::Done { .. });
            let pointer_global = pointer.map(|pos| pos + tile_rect.min.to_vec2());
            let hover = pointer_global.map(|pos| overlay.find_window_at(pos).unwrap_or(tile_rect));
            if let Some(pos) = pointer_global
                && (!over_ui || busy)
                && !ocr_open
            {
                handle_selection_input(
                    &mut overlay,
                    pos,
                    pressed,
                    held,
                    released,
                    hover,
                    tile_rect,
                );
                handle_draw_input(&mut overlay, pos, pressed, released);
                update_cursor(&ctx, &overlay, pos);
            } else if released {
                overlay.origin = None;
                overlay.draw_origin = None;
                overlay.handle = None;
                overlay.move_ref = None;
            }

            // ---- 渲染：遮罩、选区、标注 ----
            render_scene(&painter, &overlay, tile_rect, scale, pointer);

            // 选区所在屏幕才显示工具栏 / 文本框 / OCR 面板
            let sel_here = overlay
                .selection
                .filter(|sel| tile_rect.contains(sel.center()));
            if let Some(sel) = sel_here {
                draw_ocr_highlights(&painter, &overlay, sel, tile_rect);
                if !ocr_open {
                    let toolbar_rect = toolbar_rect_for(&ctx, sel, tile_rect);
                    let clicked = draw_toolbar(ui, &mut overlay, toolbar_rect, tile_rect);
                    if clicked != ToolAction::None {
                        action = clicked;
                        action_sel = Some(sel);
                    }
                }
            }

            // ---- 文本输入框 ----
            let mut esc_consumed = false;
            if let Some(anchor) = overlay.text_pos
                && tile_rect.contains(anchor)
            {
                let size = text_size(THICKNESSES[overlay.thickness]);
                let color = color32(COLORS[overlay.color]);
                let mut commit = false;
                let mut cancel = false;
                egui::Area::new(egui::Id::new("screenshot-text-input"))
                    .order(egui::Order::Foreground)
                    .constrain(false)
                    .fixed_pos(anchor - tile_rect.min.to_vec2())
                    .show(ui, |ui| {
                        let output = egui::TextEdit::singleline(&mut overlay.text_buf)
                            .desired_width(240.0)
                            .font(egui::FontId::proportional(size))
                            .text_color(color)
                            .hint_text("输入文字，Enter 确认")
                            .margin(egui::Margin::ZERO)
                            .frame(egui::Frame::NONE.stroke(egui::Stroke::new(
                                1.0_f32,
                                egui::Color32::from_white_alpha(160),
                            )))
                            .show(ui);
                        // TextEdit 已处理过按键；这里吃掉 Enter / Esc，避免同帧触发
                        // 「复制并结束」或「关闭覆盖层」
                        commit = ui
                            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                        cancel = ui
                            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                        if !commit && !cancel && !output.response.has_focus() {
                            output.response.request_focus();
                        }
                    });
                if commit {
                    overlay.commit_text();
                } else if cancel {
                    overlay.text_pos = None;
                    overlay.text_buf.clear();
                    esc_consumed = true;
                }
            }

            // ---- OCR 结果面板 ----
            let mut finish = false;
            if let Some(sel) = sel_here {
                match draw_ocr_panel(ui, &mut overlay, sel, tile_rect) {
                    OcrAction::None => {}
                    OcrAction::Close => overlay.ocr = OcrState::Idle,
                    OcrAction::CopySelected => {
                        let text = ocr_text(&overlay.ocr, true);
                        if text.is_empty() {
                            overlay.show_toast("请先选择要复制的文字");
                        } else {
                            match arboard_copy_text(text) {
                                Ok(()) => overlay.show_toast("文字已复制"),
                                Err(error) => overlay.show_toast(format!("复制失败：{error}")),
                            }
                        }
                    }
                    OcrAction::CopyAll => match arboard_copy_text(ocr_text(&overlay.ocr, false)) {
                        Ok(()) => finish = true,
                        Err(error) => overlay.show_toast(format!("复制失败：{error}")),
                    },
                }
            }

            // ---- 提示 toast / 识别中 ----
            if sel_here.is_some() || overlay.selection.is_none() {
                draw_toast(ui, &mut overlay);
            }

            // ---- 快捷键 ----
            let text_editing = overlay.text_pos.is_some();
            if !esc_consumed
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                // Esc 逐级退出：OCR 面板 → 覆盖层
                if ocr_open {
                    overlay.ocr = OcrState::Idle;
                } else {
                    finish = true;
                }
            }
            if !text_editing {
                if let Some(sel) = sel_here
                    && !ocr_open
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                {
                    action = ToolAction::Copy;
                    action_sel = Some(sel);
                }
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
                    overlay.shapes.pop();
                }
            }
            if finish {
                overlay.hide_all(&ctx);
            }
        });

    // ---- 锁外输出：复制 / 保存 / OCR / 贴图 ----
    let Some(sel) = action_sel else {
        return;
    };
    let finish = match action {
        ToolAction::None => false,
        ToolAction::Cancel => true,
        ToolAction::Copy => copy_selection(state, display_id, sel),
        ToolAction::Save => save_selection(state, display_id, sel),
        ToolAction::Pin => create_pin(&ctx, state, display_id, sel),
        ToolAction::Ocr => {
            start_ocr(&ctx, state, display_id, sel);
            false
        }
    };
    if finish {
        state.lock().unwrap().hide_all(&ctx);
    }
}

/// 复制合成后的选区到剪贴板；成功返回 true（随后结束会话）。
fn copy_selection(state: &Arc<Mutex<Overlay>>, display_id: u32, sel: egui::Rect) -> bool {
    let Some((w, h, rgba)) = compose(state, display_id, sel) else {
        return false;
    };
    match mymac_core::clipboard::copy_image_to_clipboard(w, h, rgba) {
        Ok(()) => true,
        Err(error) => {
            diag(format!("复制截图失败：{error}"));
            state
                .lock()
                .unwrap()
                .show_toast(format!("复制失败：{error}"));
            false
        }
    }
}

/// 弹出保存面板写入 PNG；保存成功返回 true（随后结束会话），取消或失败返回 false。
fn save_selection(state: &Arc<Mutex<Overlay>>, display_id: u32, sel: egui::Rect) -> bool {
    let Some((w, h, rgba)) = compose(state, display_id, sel) else {
        return false;
    };
    let name = format!(
        "MyMac截图_{}.png",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    // 保存面板（模态层级 8）会被 screenSaver 级覆盖层挡住：弹出前先降到普通层级
    crate::macos::set_level_titled_prefix("screenshot-overlay", 0);
    let picked = rfd::FileDialog::new()
        .set_title("保存截图")
        .set_file_name(&name)
        .add_filter("PNG 图片", &["png"])
        .save_file();
    crate::macos::raise_windows_titled_prefix("screenshot-overlay");
    let Some(path) = picked else {
        return false;
    };
    let written = capture::encode_png(rgba, w, h)
        .and_then(|png| std::fs::write(&path, png).map_err(|e| e.to_string()));
    match written {
        Ok(()) => {
            diag(format!("截图已保存：{}", path.display()));
            true
        }
        Err(error) => {
            diag(format!("保存截图失败：{error}"));
            state
                .lock()
                .unwrap()
                .show_toast(format!("保存失败：{error}"));
            false
        }
    }
}

/// 后台识别选区文字（不含标注）。识别期间显示「识别中」，结果只回写到同一会话。
fn start_ocr(ctx: &egui::Context, state: &Arc<Mutex<Overlay>>, display_id: u32, sel: egui::Rect) {
    let (session, crop) = {
        let mut overlay = state.lock().unwrap();
        if matches!(overlay.ocr, OcrState::Loading) {
            return;
        }
        let crop = overlay
            .tiles
            .iter()
            .find(|t| t.display_id == display_id)
            .and_then(|tile| crop_selection(tile, sel));
        if crop.is_some() {
            overlay.ocr = OcrState::Loading;
            overlay.toast = None;
        }
        (overlay.session, crop)
    };
    let Some((w, h, rgba)) = crop else {
        return;
    };
    let state = state.clone();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let result = mymac_core::ocr::recognize_rgba(rgba, w as u32, h as u32);
        {
            let mut overlay = state.lock().unwrap();
            if overlay.session != session || !matches!(overlay.ocr, OcrState::Loading) {
                return;
            }
            match result {
                Ok(result) if result.lines.is_empty() => {
                    overlay.ocr = OcrState::Idle;
                    overlay.show_toast("未识别到文字");
                }
                Ok(result) => {
                    overlay.ocr = OcrState::Done {
                        lines: result.lines,
                        selected: BTreeSet::new(),
                        hover: None,
                    };
                }
                Err(error) => {
                    overlay.ocr = OcrState::Idle;
                    overlay.show_toast(format!("文字识别失败：{error}"));
                }
            }
        }
        ctx.request_repaint_of(viewport_id(display_id));
    });
}

/// 把选区合成为贴图；成功返回 true（随后关闭覆盖层露出贴图）。
fn create_pin(
    ctx: &egui::Context,
    state: &Arc<Mutex<Overlay>>,
    display_id: u32,
    sel: egui::Rect,
) -> bool {
    let Some((w, h, rgba)) = compose(state, display_id, sel) else {
        return false;
    };
    let mut overlay = state.lock().unwrap();
    let scale = overlay
        .tiles
        .iter()
        .find(|t| t.display_id == display_id)
        .map_or(1.0, |t| t.scale) as f32;
    overlay.pin_seq += 1;
    let id = overlay.pin_seq;
    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
    let texture = ctx.load_texture(
        format!("screenshot-pin-{id}"),
        image,
        egui::TextureOptions::LINEAR,
    );
    let base = pin_base_size(w, h, scale);
    overlay.pins.push(PinImage {
        id,
        texture,
        rgba: Arc::new(rgba),
        px_size: [w, h],
        pos: sel.min,
        base,
        zoom: 1.0,
        drag_ref: None,
    });
    diag(format!("贴图已创建 id={id} size={base:?}"));
    true
}

/// 选区交互：新建 / 8 控制点调整 / 整体拖动（仅「选择」工具下生效）。
/// 选区限制在当前屏幕内（暂不支持跨屏选区）。
#[allow(clippy::too_many_arguments)]
fn handle_selection_input(
    overlay: &mut Overlay,
    pos: egui::Pos2,
    pressed: bool,
    held: bool,
    released: bool,
    hover: Option<egui::Rect>,
    tile_rect: egui::Rect,
) {
    if overlay.tool != Tool::Select {
        return;
    }
    let pos = pos.clamp(tile_rect.min, tile_rect.max);
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
        overlay.ocr = OcrState::Idle;
    }
    if held
        && let Some(sel) = overlay.selection
        && let Some(handle) = overlay.handle
    {
        overlay.selection = Some(apply_handle(sel, handle, pos));
    } else if held && let Some((start, base)) = overlay.move_ref {
        let moved = base.translate(pos - start);
        // 整体拖动时保持在屏幕内
        let shift = egui::vec2(
            (tile_rect.min.x - moved.min.x).max(0.0) + (tile_rect.max.x - moved.max.x).min(0.0),
            (tile_rect.min.y - moved.min.y).max(0.0) + (tile_rect.max.y - moved.max.y).min(0.0),
        );
        overlay.selection = Some(moved.translate(shift));
    } else if (held || released)
        && let Some(origin) = overlay.origin
    {
        let rect = egui::Rect::from_two_pos(origin, pos);
        overlay.selection = Some(rect);
        if released {
            if rect.width() < 4.0 || rect.height() < 4.0 {
                // 几乎没有拖动 → 视为单击，选中悬停目标（窗口或整屏）
                overlay.selection = hover
                    .map(|r| r.intersect(tile_rect))
                    .filter(|r| r.is_positive());
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
    if overlay.tool == Tool::Select {
        return;
    }
    let inside = overlay.selection.is_some_and(|s| s.contains(pos));
    if overlay.tool == Tool::Text {
        if pressed && inside {
            // 先提交上一段文本，再开始新锚点
            overlay.commit_text();
            overlay.text_pos = Some(pos);
        }
        return;
    }
    if pressed && inside && overlay.draw_origin.is_none() {
        overlay.draw_origin = Some(pos);
    }
    if released && let Some(origin) = overlay.draw_origin.take() {
        let rect = egui::Rect::from_two_pos(origin, pos);
        if rect.width() > 2.0 || rect.height() > 2.0 {
            overlay.shapes.push(Shape {
                tool: overlay.tool,
                rect: shape_rect(overlay.tool, origin, pos),
                text: String::new(),
                color: COLORS[overlay.color],
                width: THICKNESSES[overlay.thickness],
            });
        }
    }
}

/// 标注形状的范围：箭头保留方向（min=起点、max=终点，不规范化），其余规范化。
fn shape_rect(tool: Tool, origin: egui::Pos2, pos: egui::Pos2) -> egui::Rect {
    if tool == Tool::Arrow {
        egui::Rect::from_min_max(origin, pos)
    } else {
        egui::Rect::from_two_pos(origin, pos)
    }
}

/// 按工具与指针位置设置光标形状。
fn update_cursor(ctx: &egui::Context, overlay: &Overlay, pos: egui::Pos2) {
    use egui::CursorIcon;
    let icon = match overlay.tool {
        Tool::Select => match overlay.selection {
            Some(sel) => match overlay.handle.or_else(|| handle_at(sel, pos)) {
                Some(Handle::Nw | Handle::Se) => CursorIcon::ResizeNwSe,
                Some(Handle::Ne | Handle::Sw) => CursorIcon::ResizeNeSw,
                Some(Handle::N | Handle::S) => CursorIcon::ResizeVertical,
                Some(Handle::E | Handle::W) => CursorIcon::ResizeHorizontal,
                None if overlay.move_ref.is_some() => CursorIcon::Grabbing,
                None if sel.contains(pos) => CursorIcon::Move,
                None => CursorIcon::Crosshair,
            },
            None => CursorIcon::Crosshair,
        },
        Tool::Text => CursorIcon::Text,
        Tool::Rect | Tool::Arrow => CursorIcon::Crosshair,
    };
    ctx.set_cursor_icon(icon);
}

/// 场景渲染：遮罩、选区边框与控制点、已提交与进行中的标注、悬停高亮。
fn render_scene(
    painter: &egui::Painter,
    overlay: &Overlay,
    tile_rect: egui::Rect,
    scale: f64,
    pointer: Option<egui::Pos2>,
) {
    let offset = -tile_rect.min.to_vec2();
    let to_local = |r: egui::Rect| r.intersect(tile_rect).translate(offset);
    let screen = tile_rect.translate(offset);
    let dim = egui::Color32::from_black_alpha(110);

    let Some(sel) = overlay.selection else {
        // 无选区：整屏轻遮罩 + 高亮悬停目标（窗口 / 整屏）
        if let Some(pos) = pointer {
            let global = pos + tile_rect.min.to_vec2();
            let hover = overlay.find_window_at(global).unwrap_or(tile_rect);
            let local = to_local(hover);
            dim_outside(painter, screen, local, dim);
            if local.is_positive() {
                painter.rect_stroke(
                    local,
                    0.0,
                    egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(59, 130, 246)),
                    egui::StrokeKind::Inside,
                );
            }
        } else {
            painter.rect_filled(screen, 0.0, dim);
        }
        return;
    };

    let local = to_local(sel);
    dim_outside(painter, screen, local, dim);
    if !local.is_positive() {
        return;
    }

    // 标注（裁剪到选区）
    let shape_painter = painter.with_clip_rect(local);
    for shape in &overlay.shapes {
        draw_shape_shape(&shape_painter, shape, offset);
    }
    if let (Some(origin), Some(pos)) = (overlay.draw_origin, pointer) {
        let global = pos + tile_rect.min.to_vec2();
        let preview = Shape {
            tool: overlay.tool,
            rect: shape_rect(overlay.tool, origin, global),
            text: String::new(),
            color: COLORS[overlay.color],
            width: THICKNESSES[overlay.thickness],
        };
        draw_shape_shape(&shape_painter, &preview, offset);
    }

    // 选区边框 + 控制点 + 尺寸提示
    painter.rect_stroke(
        local,
        0.0,
        egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(59, 130, 246)),
        egui::StrokeKind::Outside,
    );
    for (_, point) in handle_points(sel) {
        let handle = egui::Rect::from_center_size(point + offset, egui::vec2(8.0, 8.0));
        painter.rect_filled(handle, 2, egui::Color32::WHITE);
        painter.rect_stroke(
            handle,
            2,
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(59, 130, 246)),
            egui::StrokeKind::Inside,
        );
    }
    let pw = (sel.width() * scale as f32).round() as u32;
    let ph = (sel.height() * scale as f32).round() as u32;
    let label = format!("{pw} × {ph}");
    let galley = painter.layout_no_wrap(
        label,
        egui::FontId::proportional(12.0),
        egui::Color32::WHITE,
    );
    let mut badge_pos = local.left_top() + egui::vec2(0.0, -galley.size().y - 10.0);
    if badge_pos.y < screen.min.y + 2.0 {
        badge_pos = local.left_top() + egui::vec2(6.0, 6.0);
    }
    let badge = egui::Rect::from_min_size(badge_pos, galley.size() + egui::vec2(10.0, 4.0));
    painter.rect_filled(badge, 4, egui::Color32::from_black_alpha(180));
    painter.galley(
        badge.min + egui::vec2(5.0, 2.0),
        galley,
        egui::Color32::WHITE,
    );
}

/// 选区外四块遮罩（选区内保持原样亮度）。
fn dim_outside(
    painter: &egui::Painter,
    screen: egui::Rect,
    hole: egui::Rect,
    color: egui::Color32,
) {
    if !hole.is_positive() {
        painter.rect_filled(screen, 0.0, color);
        return;
    }
    let parts = [
        egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, hole.min.y)),
        egui::Rect::from_min_max(egui::pos2(screen.min.x, hole.max.y), screen.max),
        egui::Rect::from_min_max(
            egui::pos2(screen.min.x, hole.min.y),
            egui::pos2(hole.min.x, hole.max.y),
        ),
        egui::Rect::from_min_max(
            egui::pos2(hole.max.x, hole.min.y),
            egui::pos2(screen.max.x, hole.max.y),
        ),
    ];
    for part in parts {
        if part.is_positive() {
            painter.rect_filled(part, 0.0, color);
        }
    }
}

/// 绘制单个标注形状（egui 画笔，用于实时预览与已提交形状）。
fn draw_shape_shape(painter: &egui::Painter, shape: &Shape, offset: egui::Vec2) {
    let color = color32(shape.color);
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
            if start.distance(end) < 1.0 {
                return;
            }
            let dir = (end - start).normalized();
            let len = shape.width * 4.0 + 6.0;
            let left = egui::pos2(
                end.x - (dir.x * 0.866 - dir.y * 0.5) * len,
                end.y - (dir.y * 0.866 + dir.x * 0.5) * len,
            );
            let right = egui::pos2(
                end.x - (dir.x * 0.866 + dir.y * 0.5) * len,
                end.y - (dir.y * 0.866 - dir.x * 0.5) * len,
            );
            // 杆身止于箭头底边，避免粗线顶出尖端
            let base = egui::pos2((left.x + right.x) / 2.0, (left.y + right.y) / 2.0);
            painter.line_segment([start, base], stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![end, left, right],
                color,
                egui::Stroke::NONE,
            ));
        }
        Tool::Text => {
            painter.text(
                shape.rect.min + offset,
                egui::Align2::LEFT_TOP,
                &shape.text,
                egui::FontId::proportional(text_size(shape.width)),
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

/// 工具栏位置（全局逻辑坐标）：选区下方右对齐，放不下时放选区上方，再不行贴屏幕顶部。
/// 尺寸取上一帧实际布局结果（首帧用估计值）。
fn toolbar_rect_for(ctx: &egui::Context, sel: egui::Rect, tile_rect: egui::Rect) -> egui::Rect {
    const GAP: f32 = 12.0;
    const MARGIN: f32 = 8.0;
    if !sel.is_finite() || !tile_rect.is_finite() {
        return egui::Rect::NOTHING;
    }
    let size = ctx
        .memory(|m| m.area_rect(egui::Id::new("screenshot-toolbar")))
        .map_or(egui::vec2(600.0, 44.0), |r| r.size());
    let mut top = sel.max.y + GAP;
    if top + size.y > tile_rect.max.y - MARGIN {
        top = sel.min.y - size.y - GAP;
    }
    top = top.max(tile_rect.min.y + MARGIN);
    let left = (sel.max.x - size.x)
        .min(tile_rect.max.x - size.x - MARGIN)
        .max(tile_rect.min.x + MARGIN);
    egui::Rect::from_min_size(egui::pos2(left, top), size)
}

/// 渲染工具栏，返回触发的动作。
#[allow(clippy::too_many_lines)]
fn draw_toolbar(
    ui: &mut egui::Ui,
    overlay: &mut Overlay,
    toolbar_rect: egui::Rect,
    tile_rect: egui::Rect,
) -> ToolAction {
    use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
    use shadcn_egui::icons::lucide;

    let mut action = ToolAction::None;
    if !toolbar_rect.is_finite() {
        return action;
    }
    let ocr_loading = matches!(overlay.ocr, OcrState::Loading);
    let palette = shadcn_egui::Theme::current(ui.ctx()).palette;
    egui::Area::new(egui::Id::new("screenshot-toolbar"))
        .order(egui::Order::Foreground)
        .constrain(false)
        .fixed_pos(toolbar_rect.min - tile_rect.min.to_vec2())
        .show(ui.ctx(), |ui| {
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
                                .icon(icon);
                            if ui.add(button).on_hover_text(tip).clicked() {
                                overlay.commit_text();
                                overlay.tool = tool;
                            }
                        };
                        tool_button(ui, Tool::Select, &lucide::MOVE, "选择 / 调整选区");
                        tool_button(ui, Tool::Rect, &lucide::SQUARE, "矩形标注");
                        tool_button(ui, Tool::Arrow, &lucide::ARROW_UP_RIGHT, "箭头标注");
                        tool_button(ui, Tool::Text, &lucide::TYPE, "文本标注");
                        ui.separator();
                        // 颜色板
                        for (index, color) in COLORS.iter().enumerate() {
                            let (rect, response) = ui
                                .allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                            ui.painter().rect_filled(rect, 4.0, color32(*color));
                            let ring = if overlay.color == index {
                                egui::Stroke::new(2.0_f32, palette.primary)
                            } else {
                                egui::Stroke::new(1.0_f32, palette.border)
                            };
                            ui.painter()
                                .rect_stroke(rect, 4.0, ring, egui::StrokeKind::Outside);
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
                                    .icon(&lucide::UNDO)
                                    .enabled(!overlay.shapes.is_empty()),
                            )
                            .on_hover_text("撤销标注（⌘Z）")
                            .clicked()
                        {
                            overlay.shapes.pop();
                        }
                        let ocr_icon = if ocr_loading {
                            &lucide::LOADER_CIRCLE
                        } else {
                            &lucide::SCAN_TEXT
                        };
                        if ui
                            .add(
                                Button::new(if ocr_loading { "识别中" } else { "识别" })
                                    .variant(ButtonVariant::Outline)
                                    .size(ButtonSize::Sm)
                                    .icon(ocr_icon)
                                    .enabled(!ocr_loading),
                            )
                            .on_hover_text("识别选区文字")
                            .clicked()
                        {
                            action = ToolAction::Ocr;
                        }
                        if ui
                            .add(
                                Button::new("贴图")
                                    .variant(ButtonVariant::Outline)
                                    .size(ButtonSize::Sm)
                                    .icon(&lucide::PIN),
                            )
                            .on_hover_text("贴图：置顶显示，拖动移动 / 滚轮缩放 / 双击或 Esc 关闭")
                            .clicked()
                        {
                            action = ToolAction::Pin;
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
                        if ui
                            .add(Button::new("复制").size(ButtonSize::Sm).icon(&lucide::COPY))
                            .on_hover_text("复制到剪贴板（Enter）")
                            .clicked()
                        {
                            action = ToolAction::Copy;
                        }
                    });
                });
        });
    // 有按钮被点击时先提交正在输入的文字，保证它进入输出
    if action != ToolAction::None {
        overlay.commit_text();
    }
    action
}

/// 识别行在选区上的位置（Vision 归一化坐标，原点左下 → 全局逻辑坐标）。
fn ocr_line_rect(line: &mymac_core::ocr::OcrLine, sel: egui::Rect) -> egui::Rect {
    let [bx, by, bw, bh] = line.bbox;
    egui::Rect::from_min_size(
        egui::pos2(
            sel.min.x + bx * sel.width(),
            sel.min.y + (1.0 - by - bh) * sel.height(),
        ),
        egui::vec2(bw * sel.width(), bh * sel.height()),
    )
}

/// OCR 结果模式下在选区上标出各识别行（选中绿色、悬停蓝色）。
fn draw_ocr_highlights(
    painter: &egui::Painter,
    overlay: &Overlay,
    sel: egui::Rect,
    tile_rect: egui::Rect,
) {
    let OcrState::Done {
        lines,
        selected,
        hover,
    } = &overlay.ocr
    else {
        return;
    };
    for (index, line) in lines.iter().enumerate() {
        let rect = ocr_line_rect(line, sel)
            .expand(2.0)
            .translate(-tile_rect.min.to_vec2());
        let (fill, stroke) = if selected.contains(&index) {
            (
                egui::Color32::from_rgba_unmultiplied(48, 164, 108, 60),
                egui::Color32::from_rgb(48, 164, 108),
            )
        } else if *hover == Some(index) {
            (
                egui::Color32::from_rgba_unmultiplied(59, 130, 246, 60),
                egui::Color32::from_rgb(59, 130, 246),
            )
        } else {
            (
                egui::Color32::from_rgba_unmultiplied(59, 130, 246, 24),
                egui::Color32::from_rgba_unmultiplied(59, 130, 246, 120),
            )
        };
        painter.rect_filled(rect, 2, fill);
        painter.rect_stroke(
            rect,
            2,
            egui::Stroke::new(1.0_f32, stroke),
            egui::StrokeKind::Outside,
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OcrAction {
    None,
    Close,
    CopySelected,
    CopyAll,
}

/// OCR 结果面板：逐行点击多选、复制选中、复制全部并结束、关闭（Esc）。
/// 放在选区右侧，放不下时放左侧，再不行贴屏幕右边。
#[allow(clippy::too_many_lines)]
fn draw_ocr_panel(
    ui: &mut egui::Ui,
    overlay: &mut Overlay,
    sel: egui::Rect,
    tile_rect: egui::Rect,
) -> OcrAction {
    use shadcn_egui::components::button::{Button, ButtonSize, ButtonVariant};
    use shadcn_egui::icons::lucide;
    const WIDTH: f32 = 340.0;
    const GAP: f32 = 16.0;

    let OcrState::Done {
        lines,
        selected,
        hover,
    } = &mut overlay.ocr
    else {
        return OcrAction::None;
    };
    let mut action = OcrAction::None;
    let palette = shadcn_egui::Theme::current(ui.ctx()).palette;
    let max_height = (tile_rect.height() - 2.0 * GAP).min(560.0);
    let x = if sel.max.x + GAP + WIDTH <= tile_rect.max.x - GAP {
        sel.max.x + GAP
    } else if sel.min.x - GAP - WIDTH >= tile_rect.min.x + GAP {
        sel.min.x - GAP - WIDTH
    } else {
        tile_rect.max.x - GAP - WIDTH
    };
    let y = sel
        .min
        .y
        .clamp(tile_rect.min.y + GAP, tile_rect.max.y - GAP - 200.0);
    let mut new_hover = None;
    egui::Area::new(egui::Id::new("screenshot-ocr-panel"))
        .order(egui::Order::Foreground)
        .constrain(true)
        .fixed_pos(egui::pos2(x, y) - tile_rect.min.to_vec2())
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(palette.card)
                .stroke(egui::Stroke::new(1.0_f32, palette.border))
                .corner_radius(10)
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(WIDTH - 24.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("识别结果").strong());
                        ui.label(
                            RichText::new(format!("{} 行", lines.len()))
                                .small()
                                .color(palette.muted_foreground),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                                action = OcrAction::Close;
                            }
                        });
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(max_height - 110.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (index, line) in lines.iter().enumerate() {
                                let checked = selected.contains(&index);
                                let response = ui.selectable_label(
                                    checked,
                                    RichText::new(&line.text).size(13.0),
                                );
                                if response.hovered() {
                                    new_hover = Some(index);
                                }
                                if response.clicked() && !selected.remove(&index) {
                                    selected.insert(index);
                                }
                            }
                        });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if selected.is_empty() {
                                "点击行可多选".to_string()
                            } else {
                                format!("已选 {} 行", selected.len())
                            })
                            .small()
                            .color(palette.muted_foreground),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    Button::new("复制全部")
                                        .size(ButtonSize::Sm)
                                        .icon(&lucide::CLIPBOARD_COPY),
                                )
                                .on_hover_text("复制全部文字并结束截图")
                                .clicked()
                            {
                                action = OcrAction::CopyAll;
                            }
                            if ui
                                .add(
                                    Button::new("复制选中")
                                        .variant(ButtonVariant::Outline)
                                        .size(ButtonSize::Sm)
                                        .icon(&lucide::COPY),
                                )
                                .clicked()
                            {
                                action = OcrAction::CopySelected;
                            }
                        });
                    });
                });
        });
    *hover = new_hover;
    action
}

/// 拼接识别文字：`only_selected` 时只取选中行（按行顺序）。
fn ocr_text(ocr: &OcrState, only_selected: bool) -> String {
    let OcrState::Done {
        lines, selected, ..
    } = ocr
    else {
        return String::new();
    };
    lines
        .iter()
        .enumerate()
        .filter(|(index, _)| !only_selected || selected.contains(index))
        .map(|(_, line)| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 屏幕底部居中的提示 toast；识别中显示加载态。
fn draw_toast(ui: &mut egui::Ui, overlay: &mut Overlay) {
    let loading = matches!(overlay.ocr, OcrState::Loading);
    if let Some((_, since)) = &overlay.toast
        && since.elapsed().as_secs_f64() > TOAST_SECS
    {
        overlay.toast = None;
    }
    let message = match (&overlay.toast, loading) {
        (Some((text, _)), _) => text.clone(),
        (None, true) => "正在识别文字…".to_string(),
        (None, false) => return,
    };
    let palette = shadcn_egui::Theme::current(ui.ctx()).palette;
    egui::Area::new(egui::Id::new("screenshot-toast"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -48.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(palette.popover)
                .stroke(egui::Stroke::new(1.0_f32, palette.border))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if loading && overlay.toast.is_none() {
                            ui.spinner();
                        }
                        ui.label(RichText::new(message).color(palette.foreground));
                    });
                });
        });
    // 到期自动消失 / 加载动画都需要持续重绘
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(100));
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
                        x,
                        y,
                        &shape.text,
                        text_size(shape.width) * scale,
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

/// 贴图视口每帧渲染与交互：拖动移动、滚轮缩放（每格 ×1.1）、
/// 双击 / Esc / 悬停关闭按钮关闭、右键菜单（复制到剪贴板 / 关闭贴图）。
#[allow(clippy::too_many_lines)]
fn pin_viewport(pin_id: u32, state: &Arc<Mutex<Overlay>>, ui: &mut egui::Ui) {
    use shadcn_egui::icons::lucide;
    let ctx = ui.ctx().clone();
    // 快照本贴图的纹理与位置（贴图可能已被关闭）
    let snapshot = {
        let overlay = state.lock().unwrap();
        overlay
            .pins
            .iter()
            .find(|p| p.id == pin_id)
            .map(|p| (p.texture.id(), p.pos))
    };
    let Some((texture_id, pin_pos)) = snapshot else {
        ctx.send_viewport_cmd_to(pin_viewport_id(pin_id), egui::ViewportCommand::Close);
        return;
    };
    let mut close = false;
    let mut copy = false;

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| {
            let rect = ui.max_rect();
            let response = ui.interact(
                rect,
                egui::Id::new(("screenshot-pin-body", pin_id)),
                egui::Sense::click_and_drag(),
            );
            let painter = ui.painter();
            painter.image(
                texture_id,
                rect,
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(59, 130, 246)),
                egui::StrokeKind::Inside,
            );

            // 悬停时右上角显示关闭按钮
            let close_rect = egui::Rect::from_center_size(
                rect.right_top() + egui::vec2(-14.0, 14.0),
                egui::vec2(20.0, 20.0),
            );
            let pointer = ui
                .input(|i| i.pointer.hover_pos())
                .filter(|p| p.is_finite());
            let on_close = pointer.is_some_and(|p| close_rect.contains(p));
            if response.hovered() || on_close {
                painter.circle_filled(
                    close_rect.center(),
                    10.0,
                    egui::Color32::from_black_alpha(if on_close { 220 } else { 150 }),
                );
                shadcn_egui::components::icon::paint_icon(
                    painter,
                    close_rect.shrink(5.0),
                    &lucide::X,
                    egui::Color32::WHITE,
                    1.6,
                );
            }

            // 拖动移动：指针换算为「窗口位置 + 局部坐标」的全局坐标，
            // 窗口跟随光标移动时局部坐标会漂移，按全局增量移动才不抖动 / 跑飞。
            let pointer_global = ui
                .input(|i| i.pointer.interact_pos())
                .filter(|p| p.is_finite())
                .map(|p| p + pin_pos.to_vec2());
            if response.drag_started() {
                let mut overlay = state.lock().unwrap();
                if let Some(pin) = overlay.pins.iter_mut().find(|p| p.id == pin_id) {
                    pin.drag_ref = pointer_global;
                }
            } else if response.dragged() {
                let mut overlay = state.lock().unwrap();
                if let Some(pin) = overlay.pins.iter_mut().find(|p| p.id == pin_id)
                    && let (Some(start), Some(cur)) = (pin.drag_ref, pointer_global)
                {
                    pin.pos += cur - start;
                    pin.drag_ref = Some(cur);
                }
            }
            if response.drag_stopped() {
                let mut overlay = state.lock().unwrap();
                if let Some(pin) = overlay.pins.iter_mut().find(|p| p.id == pin_id) {
                    pin.drag_ref = None;
                }
                ctx.set_cursor_icon(egui::CursorIcon::Grab);
            }
            if response.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if response.hovered() && !on_close {
                ctx.set_cursor_icon(egui::CursorIcon::Grab);
            }

            // 滚轮缩放：按原始滚轮事件计格（每格 ×1.1），触控板按像素折算
            let steps: f32 = ui.input(|i| {
                i.events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::MouseWheel { unit, delta, .. } => Some(match unit {
                            egui::MouseWheelUnit::Line => delta.y,
                            egui::MouseWheelUnit::Point => delta.y / 40.0,
                            egui::MouseWheelUnit::Page => delta.y * 3.0,
                        }),
                        _ => None,
                    })
                    .sum()
            });
            if response.hovered() && steps.abs() > f32::EPSILON {
                let mut overlay = state.lock().unwrap();
                if let Some(pin) = overlay.pins.iter_mut().find(|p| p.id == pin_id) {
                    let center = pin.pos + pin.size() * 0.5;
                    pin.zoom =
                        (pin.zoom * 1.1_f32.powf(steps)).clamp(PIN_ZOOM_RANGE.0, PIN_ZOOM_RANGE.1);
                    pin.pos = center - pin.size() * 0.5;
                }
            }

            if response.clicked() && on_close {
                close = true;
            }
            if response.double_clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            response.context_menu(|ui| {
                if ui.button("复制到剪贴板").clicked() {
                    copy = true;
                    ui.close();
                }
                if ui.button("关闭贴图").clicked() {
                    close = true;
                    ui.close();
                }
            });
        });

    if copy {
        let image = {
            let overlay = state.lock().unwrap();
            overlay
                .pins
                .iter()
                .find(|p| p.id == pin_id)
                .map(|p| (p.px_size, p.rgba.clone()))
        };
        if let Some(([w, h], rgba)) = image
            && let Err(error) =
                mymac_core::clipboard::copy_image_to_clipboard(w, h, rgba.as_ref().clone())
        {
            diag(format!("贴图复制失败：{error}"));
        }
    }
    if close {
        state.lock().unwrap().pins.retain(|p| p.id != pin_id);
        diag(format!("贴图已关闭 id={pin_id}"));
        ctx.send_viewport_cmd_to(pin_viewport_id(pin_id), egui::ViewportCommand::Close);
    }
}
