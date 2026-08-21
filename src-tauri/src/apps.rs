use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tauri::Emitter;

#[derive(Serialize, Deserialize, Clone)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub version: String,
    pub size: u64,
    pub is_system: bool,
}

#[derive(Serialize, Clone)]
pub struct AppRelatedItem {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub size: u64,
    pub is_app: bool,
}

/// 列出可卸载的应用（不含系统应用）。大小在后台异步计算后通过 `app-size` 事件推送。
#[tauri::command]
pub fn list_apps(app: tauri::AppHandle) -> Vec<AppInfo> {
    let mut apps: Vec<AppInfo> = Vec::new();

    scan_dir(Path::new("/Applications"), &mut apps);
    if let Ok(home) = std::env::var("HOME") {
        let user_apps = PathBuf::from(home).join("Applications");
        scan_dir(&user_apps, &mut apps);
    }

    apps.sort_by_key(|a| a.name.to_lowercase());

    // 后台逐个计算应用大小，通过事件推送，避免阻塞列表展示
    let apps_clone = apps.clone();
    std::thread::spawn(move || {
        for mut app_info in apps_clone {
            app_info.size = path_size(Path::new(&app_info.path));
            let _ = app.emit("app-size", &app_info);
        }
    });

    apps
}

/// 扫描应用本体及用户目录中的常见关联项。
#[tauri::command]
pub fn scan_app_related(app: AppInfo) -> Result<Vec<AppRelatedItem>, String> {
    let app_path = PathBuf::from(&app.path);
    if !is_removable_app(&app_path) {
        return Err("只能扫描可卸载的应用".to_string());
    }

    let mut items = vec![build_related_item(&app_path, "应用本体", true)];
    let home = PathBuf::from(std::env::var("HOME").map_err(|_| "无法读取用户目录")?);
    let library = home.join("Library");
    let mut seen = HashSet::from([app_path]);

    let exact_candidates = [
        (library.join("Application Support").join(&app.name), "应用数据"),
        (library.join("Application Support").join(&app.id), "应用数据"),
        (library.join("Caches").join(&app.id), "缓存"),
        (library.join("Caches").join(&app.name), "缓存"),
        (library.join("Containers").join(&app.id), "容器数据"),
        (library.join("HTTPStorages").join(&app.id), "网络缓存"),
        (library.join("Logs").join(&app.name), "日志"),
        (library.join("Logs").join(&app.id), "日志"),
        (
            library
                .join("Saved Application State")
                .join(format!("{}.savedState", app.id)),
            "保存状态",
        ),
        (
            library.join("Preferences").join(format!("{}.plist", app.id)),
            "偏好设置",
        ),
    ];

    for (path, kind) in exact_candidates {
        push_related(&mut items, &mut seen, path, kind);
    }

    // 在少量标准目录中查找以 bundle id 或应用名开头的关联项。
    let scan_roots = [
        (library.join("Application Support"), "应用数据"),
        (library.join("Caches"), "缓存"),
        (library.join("Preferences"), "偏好设置"),
        (library.join("Logs"), "日志"),
        (library.join("LaunchAgents"), "启动项"),
        (library.join("WebKit"), "网页数据"),
    ];
    let name_key = normalize_key(&app.name);
    let id_key = app.id.to_lowercase();
    for (root, kind) in scan_roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_lowercase();
            let normalized = normalize_key(&file_name);
            let matches_id = !id_key.is_empty() && file_name.contains(&id_key);
            let matches_name = name_key.len() >= 4 && normalized.contains(&name_key);
            if matches_id || matches_name {
                push_related(&mut items, &mut seen, entry.path(), kind);
            }
        }
    }

    items.sort_by(|a, b| b.is_app.cmp(&a.is_app).then_with(|| a.kind.cmp(&b.kind)));
    Ok(items)
}

/// 将用户勾选的应用本体/关联项移入废纸篓。
#[tauri::command]
pub fn uninstall_app_items(paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Err("请至少选择一个要删除的项目".to_string());
    }

    let mut errors = Vec::new();
    for raw in paths {
        let path = PathBuf::from(&raw);
        if !path.exists() {
            continue;
        }
        if !is_allowed_delete_path(&path) {
            errors.push(format!("不允许删除：{raw}"));
            continue;
        }
        if let Err(error) = trash::delete(&path) {
            errors.push(format!("{raw}：{error}"));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("部分项目删除失败：\n{}", errors.join("\n")))
    }
}

fn scan_dir(dir: &Path, apps: &mut Vec<AppInfo>) {
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
            .map(|extension| extension.eq_ignore_ascii_case("app"))
            .unwrap_or(false);
        if is_app {
            apps.push(build_app_info(&path));
        }
    }
}

fn build_app_info(app_path: &Path) -> AppInfo {
    let mut name = app_path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut bundle_id = String::new();
    let mut version = String::new();

    let info_plist = app_path.join("Contents/Info.plist");
    if let Ok(value) = plist::Value::from_file(&info_plist) {
        if let Some(dict) = value.as_dictionary() {
            if let Some(value) = dict.get("CFBundleName").and_then(|value| value.as_string()) {
                name = value.to_string();
            }
            if let Some(value) = dict
                .get("CFBundleIdentifier")
                .and_then(|value| value.as_string())
            {
                bundle_id = value.to_string();
            }
            if let Some(value) = dict
                .get("CFBundleShortVersionString")
                .and_then(|value| value.as_string())
            {
                version = value.to_string();
            } else if let Some(value) = dict
                .get("CFBundleVersion")
                .and_then(|value| value.as_string())
            {
                version = value.to_string();
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
        size: 0,
        is_system: false,
    }
}

fn push_related(
    items: &mut Vec<AppRelatedItem>,
    seen: &mut HashSet<PathBuf>,
    path: PathBuf,
    kind: &str,
) {
    if path.exists() && seen.insert(path.clone()) {
        items.push(build_related_item(&path, kind, false));
    }
}

fn build_related_item(path: &Path, kind: &str, is_app: bool) -> AppRelatedItem {
    AppRelatedItem {
        path: path.to_string_lossy().to_string(),
        name: path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string()),
        kind: kind.to_string(),
        size: path_size(path),
        is_app,
    }
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_removable_app(path: &Path) -> bool {
    path.exists()
        && path.extension().is_some_and(|extension| extension == "app")
        && (path.starts_with("/Applications") || user_app_dir().is_some_and(|dir| path.starts_with(dir)))
}

fn is_allowed_delete_path(path: &Path) -> bool {
    if is_removable_app(path) {
        return true;
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return false;
    };
    path.starts_with(home.join("Library"))
}

fn user_app_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Applications"))
}

fn path_size(path: &Path) -> u64 {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() || metadata.file_type().is_symlink() {
        return metadata.len();
    }

    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        total = total.saturating_add(path_size(&entry.path()));
    }
    total
}
