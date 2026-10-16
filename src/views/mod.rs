//! 主窗口各页面。页面只读共享状态、产生用户操作，不直接碰系统调用线程。

pub mod apps;
pub mod brew;
pub mod clipboard;
pub mod dashboard;
pub mod launch;
pub mod settings;
pub mod totp;
