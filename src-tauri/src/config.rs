use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 状态栏显示配置
#[derive(Serialize, Deserialize, Clone)]
pub struct StatusConfig {
    /// 是否显示 Logo 图形
    pub show_logo: bool,
    /// 是否显示 CPU 占用
    pub show_cpu: bool,
    /// 是否显示内存占用
    pub show_memory: bool,
    /// 是否显示磁盘占用
    pub show_disk: bool,
}

impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            show_logo: true,
            show_cpu: true,
            show_memory: true,
            show_disk: false,
        }
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/MyMac/config.json")
}

pub fn load_config() -> StatusConfig {
    let path = config_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(config) = serde_json::from_str(&content) {
            return config;
        }
    }
    StatusConfig::default()
}

pub fn save_config(config: &StatusConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_status_config(state: tauri::State<crate::system::AppState>) -> StatusConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_status_config(
    state: tauri::State<crate::system::AppState>,
    config: StatusConfig,
) -> Result<(), String> {
    save_config(&config)?;
    *state.config.lock().unwrap() = config;
    Ok(())
}
