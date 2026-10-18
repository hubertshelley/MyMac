//! MyMac 业务内核：不依赖任何 UI 框架，供 Tauri 版与 egui 版共用。

pub mod apps;
pub mod brew;
pub mod clipboard;
pub mod config;
pub mod event;
pub mod launch;
pub mod ocr;
pub mod paths;
pub mod screenshot;
pub mod status;
pub mod system;
pub mod totp;
