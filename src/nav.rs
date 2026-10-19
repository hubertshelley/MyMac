//! 主窗口导航。

use shadcn_egui::Icon;
use shadcn_egui::icons::lucide;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Dashboard,
    Apps,
    Launch,
    Brew,
    Clipboard,
    Totp,
    Settings,
}

impl Page {
    /// 侧边栏顺序与旧版一致
    pub const ALL: [Page; 7] = [
        Page::Dashboard,
        Page::Apps,
        Page::Launch,
        Page::Brew,
        Page::Clipboard,
        Page::Totp,
        Page::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Page::Dashboard => "资源监控",
            Page::Apps => "应用卸载",
            Page::Launch => "启动项",
            Page::Brew => "Homebrew",
            Page::Clipboard => "粘贴板",
            Page::Totp => "2FA 验证码",
            Page::Settings => "设置",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Page::Dashboard => lucide::GAUGE,
            Page::Apps => lucide::APP_WINDOW,
            Page::Launch => lucide::ROCKET,
            Page::Brew => lucide::BEER,
            Page::Clipboard => lucide::CLIPBOARD_LIST,
            Page::Totp => lucide::KEY_ROUND,
            Page::Settings => lucide::SETTINGS,
        }
    }
}
