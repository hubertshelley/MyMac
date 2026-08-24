use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

/// 单条剪贴板历史记录
#[derive(Serialize, Deserialize, Clone)]
pub struct ClipItem {
    pub id: String,
    pub content: String,
    pub created_at: String,
}

/// 剪贴板历史内存状态
pub struct ClipboardState {
    pub items: Mutex<Vec<ClipItem>>,
    /// 最近一次监听线程见过的剪贴板内容，用于区分新复制与残留内容
    pub last_seen: Mutex<Option<String>>,
}

/// 历史记录上限
const MAX_ITEMS: usize = 200;
/// 状态栏菜单展示条数
pub const TRAY_SHOW_ITEMS: usize = 10;

fn now_str() -> String {
    chrono::Local::now()
        .naive_local()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

impl ClipItem {
    fn new(content: String) -> Self {
        Self {
            id: scru128::new_string(),
            content,
            created_at: now_str(),
        }
    }
}

fn history_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/MyMac/clipboard_history.json")
}

pub fn load_history() -> Vec<ClipItem> {
    let path = history_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(items) = serde_json::from_str(&content) {
            return items;
        }
    }
    Vec::new()
}

pub fn save_history(items: &[ClipItem]) -> Result<(), String> {
    let path = history_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(items).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

/// 将剪贴板新内容写入历史（去重、置顶、截断上限），返回是否发生变化
/// `last_seen` 记录监听线程最近见过的内容，避免清空后残留内容重新入历史
pub fn upsert(items: &mut Vec<ClipItem>, text: &str, last_seen: &mut Option<String>) -> bool {
    let changed = last_seen.as_deref() != Some(text);
    *last_seen = Some(text.to_string());
    if !changed {
        return false;
    }
    if let Some(pos) = items.iter().position(|i| i.content == text) {
        let mut item = items.remove(pos);
        item.created_at = now_str();
        items.insert(0, item);
    } else {
        items.insert(0, ClipItem::new(text.to_string()));
    }
    items.truncate(MAX_ITEMS);
    true
}

/// 复制指定记录到系统剪贴板，并将其置顶
pub fn copy_to_clipboard_and_top(state: &ClipboardState, id: &str) -> Result<(), String> {
    let content = {
        let items = state.items.lock().unwrap();
        items.iter().find(|i| i.id == id).map(|i| i.content.clone())
    };
    let content = content.ok_or("记录不存在")?;

    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(content.clone()).map_err(|e| e.to_string())?;

    *state.last_seen.lock().unwrap() = Some(content.clone());

    let mut items = state.items.lock().unwrap();
    if let Some(pos) = items.iter().position(|i| i.id == id) {
        let mut item = items.remove(pos);
        item.created_at = now_str();
        items.insert(0, item);
    }
    save_history(&items)
}

/// 清空历史，并记录当前剪贴板内容，防止残留内容被监听线程重新写入
pub fn clear_history(state: &ClipboardState) -> Result<(), String> {
    let current = arboard::Clipboard::new()
        .ok()
        .and_then(|mut cb| cb.get_text().ok());
    *state.last_seen.lock().unwrap() = current;
    let mut items = state.items.lock().unwrap();
    items.clear();
    save_history(&items)
}

/// 生成状态栏菜单展示用的单行预览文本
pub fn preview(content: &str, max_chars: usize) -> String {
    let first_line = content.lines().next().unwrap_or(content).trim();
    let mut s: String = first_line.chars().take(max_chars).collect();
    let truncated = first_line.chars().count() > max_chars || content.lines().count() > 1;
    if truncated {
        s.push('…');
    }
    if s.is_empty() {
        "（空内容）".to_string()
    } else {
        s
    }
}

#[tauri::command]
pub fn get_clip_history(state: tauri::State<ClipboardState>) -> Vec<ClipItem> {
    state.items.lock().unwrap().clone()
}

#[tauri::command]
pub fn delete_clip_item(state: tauri::State<ClipboardState>, id: String) -> Result<(), String> {
    let mut items = state.items.lock().unwrap();
    items.retain(|i| i.id != id);
    save_history(&items)
}

#[tauri::command]
pub fn clear_clip_history(state: tauri::State<ClipboardState>) -> Result<(), String> {
    clear_history(&state)
}

#[tauri::command]
pub fn copy_clip_item(state: tauri::State<ClipboardState>, id: String) -> Result<(), String> {
    copy_to_clipboard_and_top(&state, &id)
}