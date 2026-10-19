//! 应用共享状态。字段都是 `Arc`，克隆后可放心交给后台线程。
//! 沿用旧版「先克隆数据、释放锁、再更新菜单」的防死锁写法。

use std::sync::{Arc, Mutex};

use mymac_core::clipboard::{self, ClipboardState};
use mymac_core::config::{self, StatusConfig};
use mymac_core::system::{SystemInfo, SystemMonitor};
use mymac_core::totp::{self, TotpState};

#[derive(Clone)]
pub struct Shared {
    pub monitor: Arc<Mutex<SystemMonitor>>,
    pub config: Arc<Mutex<StatusConfig>>,
    pub clipboard: Arc<ClipboardState>,
    pub totp: Arc<TotpState>,
    /// 最近一次采样；界面直接读取，避免每帧触发系统调用
    pub latest: Arc<Mutex<Option<SystemInfo>>>,
}

impl Shared {
    pub fn load() -> Self {
        Self {
            monitor: Arc::new(Mutex::new(SystemMonitor::new())),
            config: Arc::new(Mutex::new(config::load_config())),
            clipboard: Arc::new(ClipboardState {
                items: Mutex::new(clipboard::load_history()),
                // 启动时不读取剪贴板内容，避免触发通用剪贴板按需取回
                last_seen: Mutex::new(None),
            }),
            // 账户列表尽量完整（密钥可能为空），错误由 2FA 页提示
            totp: Arc::new(TotpState {
                accounts: Mutex::new(totp::load_accounts()),
            }),
            latest: Arc::new(Mutex::new(None)),
        }
    }

    /// 采样并缓存，返回快照。
    pub fn sample(&self) -> SystemInfo {
        let snapshot = {
            let mut monitor = self.monitor.lock().unwrap();
            monitor.refresh();
            monitor.snapshot()
        };
        *self.latest.lock().unwrap() = Some(snapshot.clone());
        snapshot
    }
}
