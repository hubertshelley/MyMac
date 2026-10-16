//! 界面层事件：托盘菜单、全局快捷键等外部来源 → 主界面。
//!
//! 唤醒规则（M0 验证）：必须 `request_repaint_of(ViewportId::ROOT)`。
//! `App::logic` 只在根视口执行，从子视口回调里用 `request_repaint()` 只会重绘子视口。

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiEvent {
    ShowMainWindow,
    /// 启动 / 重复触发截图（覆盖层存在时重新抓屏并替换会话）
    StartScreenshot,
    Quit,
}

/// creator 回调里填入的 `egui::Context`，供后台线程唤醒界面。
pub type SharedCtx = Arc<Mutex<Option<egui::Context>>>;

/// 事件出口：发送后立即唤醒根视口的 `logic`。
#[derive(Clone)]
pub struct UiSink {
    tx: Sender<UiEvent>,
    ctx: SharedCtx,
}

impl UiSink {
    pub fn new(tx: Sender<UiEvent>, ctx: SharedCtx) -> Self {
        Self { tx, ctx }
    }

    pub fn send(&self, event: UiEvent) {
        if self.tx.send(event).is_ok()
            && let Some(ctx) = self.ctx.lock().unwrap().as_ref()
        {
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }
}
