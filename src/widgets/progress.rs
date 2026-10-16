//! shadcn Progress：圆角轨道 + 主色填充。库本身没有此控件。

use eframe::egui::{self, Response, Sense, Ui, Vec2, Widget};
use shadcn_egui::Theme;

pub struct Progress {
    /// 0.0 ..= 100.0
    value: f32,
    height: f32,
}

impl Progress {
    pub fn new(value: f32) -> Self {
        Self {
            value: value.clamp(0.0, 100.0),
            height: 8.0,
        }
    }

    /// 轨道高度（圆角自动取高度一半）
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

impl Widget for Progress {
    fn ui(self, ui: &mut Ui) -> Response {
        let palette = Theme::current(ui.ctx()).palette;
        let size = Vec2::new(ui.available_width(), self.height);
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
        if ui.is_rect_visible(rect) {
            let radius = egui::CornerRadius::same((self.height / 2.0).round() as u8);
            let painter = ui.painter();
            // 轨道：primary 的 20% 不透明度（shadcn: bg-primary/20）
            painter.rect_filled(rect, radius, palette.primary.gamma_multiply(0.2));
            let mut fill = rect;
            fill.set_width(rect.width() * self.value / 100.0);
            painter.rect_filled(fill, radius, palette.primary);
        }
        response
    }
}
