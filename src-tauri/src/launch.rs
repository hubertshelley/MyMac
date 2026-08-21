use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct LaunchItem {
    pub id: String,
    pub name: String,
    pub path: String,
    pub program: String,
    pub run_at_load: bool,
    pub enabled: bool,
    pub is_user: bool,
    pub location: String,
}

#[tauri::command]
pub fn list_launch_items() -> Vec<LaunchItem> {
    let mut items: Vec<LaunchItem> = Vec::new();
    let home = std::env::var("HOME").unwrap_or_default();

    if !home.is_empty() {
        let user_agents = PathBuf::from(&home).join("Library/LaunchAgents");
        scan_dir(&user_agents, "用户登录项", true, &mut items);
    }

    scan_dir(
        Path::new("/Library/LaunchAgents"),
        "全局登录项",
        false,
        &mut items,
    );
    scan_dir(
        Path::new("/Library/LaunchDaemons"),
        "全局守护进程",
        false,
        &mut items,
    );
    scan_dir(
        Path::new("/System/Library/LaunchAgents"),
        "系统登录项",
        false,
        &mut items,
    );
    scan_dir(
        Path::new("/System/Library/LaunchDaemons"),
        "系统守护进程",
        false,
        &mut items,
    );

    items.sort_by(|a, b| {
        let by_user = b.is_user.cmp(&a.is_user);
        by_user.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    items
}

#[tauri::command]
pub fn set_launch_item(path: String, enabled: bool) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err("启动项不存在".to_string());
    }
    if !is_user_agent(p) {
        return Err("系统级启动项需管理员权限，暂不支持修改".to_string());
    }

    let mut value = plist::Value::from_file(p).map_err(|e| format!("读取失败：{e}"))?;
    let dict = value
        .as_dictionary_mut()
        .ok_or_else(|| "无效的启动项文件".to_string())?;

    if enabled {
        dict.remove("Disabled");
    } else {
        dict.insert("Disabled".to_string(), plist::Value::Boolean(true));
    }

    value.to_file_xml(p).map_err(|e| format!("写入失败：{e}"))?;
    reload_launch_item(p, enabled);
    Ok(())
}

fn scan_dir(dir: &Path, location: &str, is_user: bool, items: &mut Vec<LaunchItem>) {
    if !dir.exists() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_plist = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("plist"))
            .unwrap_or(false);
        if is_plist {
            items.push(parse_launch_plist(&path, location, is_user));
        }
    }
}

fn parse_launch_plist(path: &Path, location: &str, is_user: bool) -> LaunchItem {
    let fallback = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut name = fallback.clone();
    let mut program = String::new();
    let mut run_at_load = false;
    let mut disabled = false;

    if let Ok(value) = plist::Value::from_file(path) {
        if let Some(dict) = value.as_dictionary() {
            if let Some(v) = dict.get("Label").and_then(|v| v.as_string()) {
                name = v.to_string();
            }
            if let Some(v) = dict.get("Program").and_then(|v| v.as_string()) {
                program = v.to_string();
            } else if let Some(v) = dict.get("ProgramArguments").and_then(|v| v.as_array()) {
                if let Some(first) = v.first().and_then(|x| x.as_string()) {
                    program = first.to_string();
                }
            }
            if let Some(v) = dict.get("RunAtLoad").and_then(|v| v.as_boolean()) {
                run_at_load = v;
            }
            if let Some(v) = dict.get("Disabled").and_then(|v| v.as_boolean()) {
                disabled = v;
            }
        }
    }

    LaunchItem {
        id: name.clone(),
        name,
        path: path.to_string_lossy().to_string(),
        program,
        run_at_load,
        enabled: !disabled,
        is_user,
        location: location.to_string(),
    }
}

fn is_user_agent(path: &Path) -> bool {
    let Ok(home) = std::env::var("HOME") else {
        return false;
    };
    let user_agents = PathBuf::from(home).join("Library/LaunchAgents");
    path.starts_with(&user_agents)
}

fn reload_launch_item(path: &Path, enabled: bool) {
    let uid = user_id();
    if uid.is_empty() {
        return;
    }
    let domain = format!("gui/{uid}");
    let path_str = path.to_string_lossy().to_string();
    let args: Vec<&str> = if enabled {
        vec!["bootstrap", &domain, &path_str]
    } else {
        vec!["bootout", &domain, &path_str]
    };
    // 立即生效失败（如原本未加载）不影响结果，忽略错误
    let _ = std::process::Command::new("launchctl")
        .args(args)
        .output();
}

fn user_id() -> String {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

#[tauri::command]
pub fn delete_launch_item(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err("启动项不存在".to_string());
    }
    if !is_user_agent(p) {
        return Err("系统级启动项需管理员权限，暂不支持删除".to_string());
    }
    // 先从 launchd 卸载，再移入废纸篓
    reload_launch_item(p, false);
    trash::delete(p).map_err(|e| format!("删除失败：{e}"))
}

#[tauri::command]
pub fn reveal_launch_item(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err("启动项文件不存在".to_string());
    }
    std::process::Command::new("open")
        .arg("-R")
        .arg(&target)
        .spawn()
        .map_err(|error| format!("打开 Finder 失败：{error}"))?;
    Ok(())
}
