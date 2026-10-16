//! 截图覆盖层：每显示器一个全屏置顶透明视口 + 冻结背景纹理。
//!
//! 交互（M5 阶段一）：
//! - 悬停高亮应用窗口；悬停无窗口区域高亮整屏（M5 新需求 1）
//! - 单击选中（窗口 / 整屏），拖拽自由框选，选区可复制
//! - Enter 复制选区到剪贴板，Esc 取消
//! - 覆盖层存在时再触发（F1 / 托盘）→ 重新抓屏替换会话（M5 新需求 2）
//!
//! 待后续阶段：标注工具、OCR 面板、保存文件、贴图视口。

use std::sync::{Arc, Mutex};

use eframe::egui;
use mymac_core::screenshot::capture::{self, DisplayInfo};
use mymac_core::screenshot::winlist::{self, CaptureWindow};

/// 单个显示器的冻结背景。
#[derive(Clone)]
pub struct Tile {
    pub display_id: u32,
    /// 全局逻辑坐标（与 CG / winit 全局坐标一致，左上原点）
    pub rect: egui::Rect,
    pub scale: f64,
    /// 物理像素 RGBA，用于选区裁剪
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

/// 覆盖层共享状态（主线程与各视口回调之间共享）。
struct Overlay {
    visible: bool,
    tiles: Vec<Tile>,
    windows: Vec<CaptureWindow>,
    /// 拖拽起点（全局逻辑坐标）
    origin: Option<egui::Pos2>,
    /// 当前选区（全局逻辑坐标）
    selection: Option<egui::Rect>,
}

impl Overlay {
    fn hide_all(&mut self, ctx: &egui::Context) {
        let ids: Vec<egui::ViewportId> = self.tiles.iter().map(|t| t.viewport_id()).collect();
        self.visible = false;
        self.origin = None;
        self.selection = None;
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
    /// 已注册覆盖层视口的显示器 id（ui 每帧按这个列表注册）
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
            })),
            displays: capture::active_displays(),
        }
    }

    /// 触发截图（F1 / 托盘；覆盖层已存在时重新抓屏替换会话）。
    /// 抓屏（CGDisplayCreateImage）必须在主线程，logic() 即主线程。
    pub fn start(&mut self, ctx: &egui::Context) {
        if !capture::has_screen_capture_access() {
            capture::request_screen_capture_access();
            log::warn!("屏幕录制权限未授予，已请求系统授权");
            return;
        }
        let displays = capture::active_displays();
        if displays.is_empty() {
            log::warn!("未枚举到任何显示器，无法截图");
            return;
        }
        self.displays = displays.clone();

        let windows = winlist::list_on_screen_windows();
        let mut tiles = Vec::with_capacity(displays.len());
        for (index, display) in displays.iter().enumerate() {
            match capture::grab_display_rgba(display.id) {
                Ok(grab) => {
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
            return;
        }

        let id_list: Vec<egui::ViewportId> = tiles.iter().map(|t| t.viewport_id()).collect();
        {
            let mut overlay = self.overlay.lock().unwrap();
            overlay.tiles = tiles;
            overlay.windows = windows;
            overlay.origin = None;
            overlay.selection = None;
            overlay.visible = true;
        }
        for id in id_list {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        log::info!("截图覆盖层已显示（{} 块屏幕）", self.displays.len());
    }

    /// 主窗口可见期间每帧注册覆盖层视口（隐藏态），显隐全部由 ViewportCommand 驱动。
    pub fn register(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
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
    }
}

/// 单个覆盖层视口的每帧渲染与交互。
fn overlay_viewport(display_id: u32, state: &Arc<Mutex<Overlay>>, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut finish = false;
    let mut copy_crop: Option<egui::Rect> = None;

    #[allow(deprecated)]
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| {
            let mut overlay = state.lock().unwrap();
            if !overlay.visible {
                return;
            }
            let Some((tile_rect, scale, texture_id)) = overlay
                .tiles
                .iter()
                .find(|t| t.display_id == display_id)
                .map(|t| (t.rect, t.scale, t.texture.id()))
            else {
                return;
            };
            let painter = ui.painter();
            // 冻结背景
            painter.image(
                texture_id,
                ui.max_rect(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, tile_rect.size()),
                egui::Color32::WHITE,
            );

            let pointer = ui.input(|i| i.pointer.interact_pos());
            let pressed = ui.input(|i| i.pointer.primary_down());
            let released = ui.input(|i| i.pointer.primary_released());

            // 悬停目标：窗口优先，无窗口则整屏（M5 新需求 1）
            let hover = pointer.and_then(|pos| {
                let global = pos + tile_rect.min.to_vec2();
                overlay.find_window_at(global).or(Some(tile_rect))
            });

            // 拖拽 / 单击选择
            if let Some(pos) = pointer
                && pressed
                && overlay.origin.is_none()
            {
                overlay.origin = Some(pos + tile_rect.min.to_vec2());
            }
            if let (Some(origin), Some(pos)) = (overlay.origin, pointer) {
                let global = pos + tile_rect.min.to_vec2();
                let rect = egui::Rect::from_two_pos(origin, global);
                overlay.selection = Some(rect);
                if released {
                    // 几乎没有拖动 → 视为单击，选中悬停目标（窗口或整屏）
                    if rect.width() < 4.0 || rect.height() < 4.0 {
                        overlay.selection = hover;
                    }
                    overlay.origin = None;
                }
            }

            // 选区遮罩：选区外压暗
            if let Some(sel) = overlay.selection {
                let full = tile_rect;
                for side in [
                    egui::Rect::from_two_pos(full.min, egui::pos2(full.max.x, sel.min.y)),
                    egui::Rect::from_two_pos(egui::pos2(full.min.x, sel.max.y), full.max),
                    egui::Rect::from_two_pos(
                        egui::pos2(full.min.x, sel.min.y),
                        egui::pos2(sel.min.x, sel.max.y),
                    ),
                    egui::Rect::from_two_pos(
                        egui::pos2(sel.max.x, sel.min.y),
                        egui::pos2(full.max.x, sel.max.y),
                    ),
                ] {
                    let local = side.intersect(full);
                    if local.is_positive() {
                        painter.rect_filled(
                            local.translate(-full.min.to_vec2()),
                            0.0,
                            egui::Color32::from_black_alpha(110),
                        );
                    }
                }
                // 选区边框 + 尺寸提示
                let local = sel.intersect(full);
                painter.rect_stroke(
                    local.translate(-full.min.to_vec2()),
                    0.0,
                    egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
                    egui::StrokeKind::Outside,
                );
                {
                    let pw = (sel.width() * scale as f32).round() as u32;
                    let ph = (sel.height() * scale as f32).round() as u32;
                    painter.text(
                        local.translate(-full.min.to_vec2()).left_top() + egui::vec2(0.0, -8.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("{pw} × {ph}"),
                        egui::FontId::proportional(12.0),
                        egui::Color32::WHITE,
                    );
                }
            } else if let Some(hover) = hover {
                // 无选区时高亮悬停目标
                let local = hover.intersect(tile_rect);
                if local.is_positive() {
                    painter.rect_filled(
                        local.translate(-tile_rect.min.to_vec2()),
                        0.0,
                        egui::Color32::from_white_alpha(28),
                    );
                    painter.rect_stroke(
                        local.translate(-tile_rect.min.to_vec2()),
                        0.0,
                        egui::Stroke::new(2.0_f32, egui::Color32::WHITE),
                        egui::StrokeKind::Inside,
                    );
                }
            }

            // 快捷键
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                finish = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Some(sel) = overlay.selection
            {
                copy_crop = Some(sel);
                finish = true;
            }
            if finish {
                overlay.hide_all(&ctx);
            }
        });

    if let (Some(sel), true) = (copy_crop, finish) {
        let crop = {
            let overlay = state.lock().unwrap();
            overlay
                .tiles
                .iter()
                .find(|t| t.display_id == display_id)
                .and_then(|tile| crop_selection(tile, sel))
        };
        if let Some((w, h, rgba)) = crop {
            // 复制放后台线程，避免阻塞
            std::thread::spawn(move || {
                if let Err(error) = mymac_core::clipboard::copy_image_to_clipboard(w, h, rgba) {
                    log::error!("复制截图失败：{error}");
                }
            });
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
