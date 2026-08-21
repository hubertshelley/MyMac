use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub version: String,
    pub size: u64,
    pub is_system: bool,
}

#[tauri::command]
pub fn list_apps() -> Vec<AppInfo> {
    let mut apps: Vec<AppInfo> = Vec::new();

    scan_dir(Path::new("/Applications"), false, &mut apps);
    scan_dir(Path::new("/System/Applications"), true, &mut apps);

    if let Ok(home) = std::env::var("HOME") {
        let user_apps = PathBuf::from(home).join("Applications");
        scan_dir(&user_apps, false, &mut apps);
    }

    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

#[tauri::command]
pub fn uninstall_app(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err("应用不存在".to_string());
    }
    if path.starts_with("/System/") {
        return Err("系统应用不允许卸载".to_string());
    }
    trash::delete(p).map_err(|e| format!("移入废纸篓失败：{e}"))
}

fn scan_dir(dir: &Path, is_system: bool, apps: &mut Vec<AppInfo>) {
    if !dir.exists() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_app = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("app"))
            .unwrap_or(false);
        if is_app {
            apps.push(build_app_info(&path, is_system));
        }
    }
}

fn build_app_info(app_path: &Path, is_system: bool) -> AppInfo {
    let mut name = app_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut bundle_id = String::new();
    let mut version = String::new();

    let info_plist = app_path.join("Contents/Info.plist");
    if let Ok(value) = plist::Value::from_file(&info_plist) {
        if let Some(dict) = value.as_dictionary() {
            if let Some(v) = dict.get("CFBundleName").and_then(|v| v.as_string()) {
                name = v.to_string();
            }
            if let Some(v) = dict.get("CFBundleIdentifier").and_then(|v| v.as_string()) {
                bundle_id = v.to_string();
            }
            if let Some(v) = dict
                .get("CFBundleShortVersionString")
                .and_then(|v| v.as_string())
            {
                version = v.to_string();
            } else if let Some(v) = dict.get("CFBundleVersion").and_then(|v| v.as_string()) {
                version = v.to_string();
            }
        }
    }

    let id = if bundle_id.is_empty() {
        app_path.to_string_lossy().to_string()
    } else {
        bundle_id
    };

    AppInfo {
        id,
        name,
        path: app_path.to_string_lossy().to_string(),
        version,
        size: dir_size(app_path),
        is_system,
    }
}

fn dir_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        // 不跟随符号链接，避免循环与重复计数
        if let Ok(meta) = entry.metadata() {
            if meta.is_dir() {
                total += dir_size(&entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}
