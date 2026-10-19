//! 页面用的后台任务：在线程里跑阻塞调用，完成后唤醒界面，页面每帧轮询结果。
//!
//! 页面代码不碰线程；`Task::spawn` 之后在 `ui` 里 `poll()` 即可。结果通过
//! `request_repaint_of(ROOT)` 唤醒根视口，所以窗口隐藏时也不会丢。

use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui;

pub struct Task<T> {
    rx: Receiver<T>,
}

impl<T: Send + 'static> Task<T> {
    pub fn spawn(ctx: &egui::Context, job: impl FnOnce() -> T + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        });
        Self { rx }
    }

    /// 完成时返回结果（只返回一次）；线程异常退出时返回 `None` 并视为已结束。
    pub fn poll(&self) -> Poll<T> {
        match self.rx.try_recv() {
            Ok(value) => Poll::Ready(value),
            Err(TryRecvError::Empty) => Poll::Pending,
            Err(TryRecvError::Disconnected) => Poll::Lost,
        }
    }
}

pub enum Poll<T> {
    Pending,
    Ready(T),
    /// 线程 panic 或被丢弃，没有结果
    Lost,
}
