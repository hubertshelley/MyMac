//! 侧边栏导航项：整行宽度、图标 + 文字左对齐。
//!
//! shadcn-egui 的 Button 内容居中且宽度随内容，做不出 shadcn Sidebar 的整行菜单项，
//! 这里按同一套色板自绘：选中态实心 `primary`（与旧版一致），悬停态 `accent`。

use eframe::egui::{self, Response, Sense, Ui, Vec2, Widget};
use shadcn_egui::components::icon::paint_icon;
use shadcn_egui::{Icon, Theme};

pub struct NavItem<'a> {
    label: &'a str,
    icon: &'a Icon,
    selected: bool,
}

impl<'a> NavItem<'a> {
    pub fn new(label: &'a str, icon: &'a Icon, selected: bool) -> Self {
        Self {
            label,
            icon,
            selected,
        }
    }
}

impl Widget for NavItem<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let theme = Theme::current(ui.ctx());
        let palette = theme.palette;
        let size = Vec2::new(ui.available_width(), 34.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, self.label));

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let radius = egui::CornerRadius::same(8);
            let (fill, fg) = if self.selected {
                (Some(palette.primary), palette.primary_foreground)
            } else if response.hovered() {
                (Some(palette.accent), palette.accent_foreground)
            } else {
                (None, palette.muted_foreground)
            };
            if let Some(fill) = fill {
                painter.rect_filled(rect, radius, fill);
            }

            let icon_size = 16.0;
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(rect.left() + 12.0 + icon_size / 2.0, rect.center().y),
                Vec2::splat(icon_size),
            );
            paint_icon(painter, icon_rect, self.icon, fg, theme.metrics.icon_stroke);

            painter.text(
                egui::pos2(icon_rect.right() + 10.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                self.label,
                egui::FontId::proportional(14.0),
                fg,
            );
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}
