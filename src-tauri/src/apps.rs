use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::Emitter;

#[derive(Serialize, Clone)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub version: String,
    pub size: u64,
    pub is_system: bool,
}

/// 列出可卸载的应用（不含系统应用）。大小在后台异步计算后通过 `app-size` 事件推送。
#[tauri::command]
pub fn list_apps(app: tauri::AppHandle) -> Vec<AppInfo> {
    let mut apps: Vec<AppInfo> = Vec::new();

    scan_dir(Path::new("/Applications"), false, &mut apps);
    if let Ok(home) = std::env::var("HOME") {
        let user_apps = PathBuf::from(home).join("Applications");
        scan_dir(&user_apps, false, &mut apps);
    }

    apps.sort_by_key(|a| a.name.to_lowercase());

    // 后台逐个计算应用大小，通过事件推送，避免阻塞列表展示
    let apps_clone = apps.clone();
    std::thread::spawn(move || {
        for mut a in apps_clone {
            a.size = dir_size(Path::new(&a.path));
            let _ = app.emit("app-size", &a);
        }
    });

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
        size: 0, // 后台异步计算
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
