//! 界面层事件：托盘菜单、全局快捷键、后台线程 → 主界面。
//!
//! 唤醒规则（M0 验证）：必须 `request_repaint_of(ViewportId::ROOT)`。
//! `App::logic` 只在根视口执行，从子视口回调里用 `request_repaint()` 只会重绘子视口。

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use eframe::egui;

use mymac_core::apps::AppInfo;

use crate::nav::Page;

#[derive(Debug, Clone)]
pub enum UiEvent {
    ShowMainWindow,
    /// 显示主窗口并切到指定页面（如托盘「管理 2FA」）
    Navigate(Page),
    /// 启动 / 重复触发截图（覆盖层存在时重新抓屏并替换会话）
    StartScreenshot,
    /// 后台线程完成一次系统采样（每 2 秒），主线程据此刷新托盘
    SystemSampled,
    /// 应用大小计算完成（`list_apps` 后台线程逐个推送）
    AppSize(AppInfo),
    /// 粘贴板历史变化（来自监听线程或托盘操作）
    ClipboardChanged,
    /// 状态栏 2FA 复制结果（成功时携带账户 id）
    TotpCopied(Result<String, String>),
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
