//! 业务层向界面层上报事件的统一出口。
//!
//! core 不依赖任何 UI 框架：后台线程只往 `EventSink` 里投递 `AppEvent`，
//! 由界面层（Tauri 或 egui）决定如何消费、如何唤醒界面。

use std::sync::Arc;

use serde::Serialize;

use crate::apps::AppInfo;
use crate::brew::BrewProgress;

/// 业务层产生、界面层消费的事件。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "payload", rename_all = "kebab-case")]
pub enum AppEvent {
    /// 应用大小计算完成（`list_apps` 后台线程逐个推送）
    AppSize(AppInfo),
    /// Homebrew 任务进度
    BrewProgress(BrewProgress),
}

/// 事件出口。克隆代价很小，可以放心传给后台线程。
#[derive(Clone)]
pub struct EventSink(Arc<dyn Fn(AppEvent) + Send + Sync>);

impl EventSink {
    pub fn new(handler: impl Fn(AppEvent) + Send + Sync + 'static) -> Self {
        Self(Arc::new(handler))
    }

    /// 丢弃所有事件，用于测试或不关心进度的调用方。
    pub fn noop() -> Self {
        Self::new(|_| {})
    }

    pub fn send(&self, event: AppEvent) {
        (self.0)(event);
    }
}

impl std::fmt::Debug for EventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EventSink")
    }
}
