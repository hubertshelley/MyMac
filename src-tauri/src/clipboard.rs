use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Mutex;

/// 记录类型
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ClipKind {
    #[default]
    Text,
    Image,
}

/// 单条剪贴板历史记录（持久化结构）
#[derive(Serialize, Deserialize, Clone)]
pub struct ClipItem {
    pub id: String,
    /// 文本内容；图片记录为空字符串
    pub content: String,
    pub created_at: String,
    #[serde(default)]
    pub kind: ClipKind,
    /// 原图文件名（图片记录）
    #[serde(default)]
    pub image_file: Option<String>,
    /// 缩略图文件名（图片记录）
    #[serde(default)]
    pub thumb_file: Option<String>,
    /// 图片宽高（图片记录）
    #[serde(default)]
    pub image_size: Option<(u32, u32)>,
    /// 图片字节指纹（用于去重）
    #[serde(default)]
    pub image_fp: Option<String>,
}

/// 返回给前端的视图结构（附带缩略图 data URL，不持久化）
#[derive(Serialize, Clone)]
pub struct ClipItemView {
    pub id: String,
    pub content: String,
    pub created_at: String,
    pub kind: ClipKind,
    pub image_size: Option<(u32, u32)>,
    pub thumbnail: Option<String>,
}

/// 剪贴板历史内存状态
pub struct ClipboardState {
    pub items: Mutex<Vec<ClipItem>>,
    /// 最近一次监听线程见过的内容指纹，用于区分新复制与残留内容
    pub last_seen: Mutex<Option<String>>,
}

/// 历史记录上限
const MAX_ITEMS: usize = 200;
/// 状态栏菜单展示条数
pub const TRAY_SHOW_ITEMS: usize = 10;
/// 缩略图最大边长
const THUMB_MAX: u32 = 256;

fn now_str() -> String {
    chrono::Local::now()
        .naive_local()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn history_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/MyMac/clipboard_history.json")
}

fn images_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/MyMac/clipboard_images")
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

/// 内容指纹：文本直接存内容，图片存尺寸 + 字节哈希
fn fingerprint_text(text: &str) -> String {
    text.to_string()
}

fn fingerprint_image(width: usize, height: usize, bytes: &[u8]) -> String {
    let mut h = DefaultHasher::new();
    width.hash(&mut h);
    height.hash(&mut h);
    bytes.hash(&mut h);
    format!("img:{}x{}:{}", width, height, h.finish())
}

/// 保存图片原图与缩略图，返回（原图文件名, 缩略图文件名, 宽高）
fn save_image_files(
    id: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(String, String, (u32, u32)), String> {
    let dir = images_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let orig_name = format!("{id}.png");
    let thumb_name = format!("{id}_thumb.png");
    let orig_path = dir.join(&orig_name);
    let thumb_path = dir.join(&thumb_name);

    image::save_buffer(&orig_path, rgba, width, height, image::ColorType::Rgba8)
        .map_err(|e| e.to_string())?;

    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or("图片数据无效")?;
    let (tw, th) = if width >= height {
        (THUMB_MAX, ((height as u64 * THUMB_MAX as u64) / width as u64) as u32)
    } else {
        (((width as u64 * THUMB_MAX as u64) / height as u64) as u32, THUMB_MAX)
    };
    let thumb = image::imageops::resize(
        &img,
        tw.max(1),
        th.max(1),
        image::imageops::FilterType::Lanczos3,
    );
    thumb.save(&thumb_path).map_err(|e| e.to_string())?;

    Ok((orig_name, thumb_name, (width, height)))
}

/// 删除记录对应的图片文件
fn remove_image_files(item: &ClipItem) {
    if item.kind != ClipKind::Image {
        return;
    }
    let dir = images_dir();
    if let Some(f) = &item.image_file {
        let _ = std::fs::remove_file(dir.join(f));
    }
    if let Some(f) = &item.thumb_file {
        let _ = std::fs::remove_file(dir.join(f));
    }
}

/// 读取缩略图并转为 data URL
fn thumbnail_data_url(file: &str) -> Option<String> {
    let bytes = std::fs::read(images_dir().join(file)).ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:image/png;base64,{b64}"))
}

/// 已保存到磁盘的图片记录（锁外预处理结果）
pub struct PreparedImage {
    pub id: String,
    pub fp: String,
    pub orig_file: String,
    pub thumb_file: String,
    pub size: (u32, u32),
}

/// 新捕获的剪贴板内容
pub enum NewContent {
    Text(String),
    Image(PreparedImage),
}

/// 计算图片内容指纹
pub fn image_fingerprint(width: usize, height: usize, bytes: &[u8]) -> String {
    fingerprint_image(width, height, bytes)
}

/// 锁外预处理：保存图片原图与缩略图（耗时操作，勿在持锁时调用）
pub fn prepare_image(width: u32, height: u32, rgba: &[u8]) -> Option<PreparedImage> {
    let id = scru128::new_string();
    let fp = fingerprint_image(width as usize, height as usize, rgba);
    let (orig, thumb, size) = save_image_files(&id, width, height, rgba).ok()?;
    Some(PreparedImage {
        id,
        fp,
        orig_file: orig,
        thumb_file: thumb,
        size,
    })
}

/// 清理预处理产生的图片文件（内容重复时调用）
fn remove_prepared_files(p: &PreparedImage) {
    let dir = images_dir();
    let _ = std::fs::remove_file(dir.join(&p.orig_file));
    let _ = std::fs::remove_file(dir.join(&p.thumb_file));
}

/// 将剪贴板新内容写入历史（去重、置顶、截断上限），返回是否发生变化
pub fn upsert(
    items: &mut Vec<ClipItem>,
    content: NewContent,
    last_seen: &mut Option<String>,
) -> bool {
    let fp = match &content {
        NewContent::Text(t) => fingerprint_text(t),
        NewContent::Image(p) => p.fp.clone(),
    };
    let changed = last_seen.as_deref() != Some(fp.as_str());
    *last_seen = Some(fp.clone());
    if !changed {
        // 内容与上次相同：若本次已保存图片文件则清理
        if let NewContent::Image(p) = &content {
            remove_prepared_files(p);
        }
        return false;
    }

    match content {
        NewContent::Text(text) => {
            if let Some(pos) = items
                .iter()
                .position(|i| i.kind == ClipKind::Text && i.content == text)
            {
                let mut item = items.remove(pos);
                item.created_at = now_str();
                items.insert(0, item);
            } else {
                items.insert(
                    0,
                    ClipItem {
                        id: scru128::new_string(),
                        content: text,
                        created_at: now_str(),
                        kind: ClipKind::Text,
                        image_file: None,
                        thumb_file: None,
                        image_size: None,
                        image_fp: None,
                    },
                );
            }
        }
        NewContent::Image(p) => {
            // 全局去重：历史中已存在相同图片则置顶，并清理本次保存的文件
            if let Some(pos) = items
                .iter()
                .position(|i| i.image_fp.as_deref() == Some(p.fp.as_str()))
            {
                remove_prepared_files(&p);
                let mut item = items.remove(pos);
                item.created_at = now_str();
                items.insert(0, item);
            } else {
                items.insert(
                    0,
                    ClipItem {
                        id: p.id,
                        content: String::new(),
                        created_at: now_str(),
                        kind: ClipKind::Image,
                        image_file: Some(p.orig_file),
                        thumb_file: Some(p.thumb_file),
                        image_size: Some(p.size),
                        image_fp: Some(p.fp),
                    },
                );
            }
        }
    }

    // 截断上限，并清理被淘汰图片的文件
    while items.len() > MAX_ITEMS {
        if let Some(removed) = items.pop() {
            remove_image_files(&removed);
        }
    }
    true
}

/// 复制指定记录到系统剪贴板，并将其置顶
pub fn copy_to_clipboard_and_top(state: &ClipboardState, id: &str) -> Result<(), String> {
    let item = {
        let items = state.items.lock().unwrap();
        items.iter().find(|i| i.id == id).cloned()
    };
    let item = item.ok_or("记录不存在")?;

    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    match item.kind {
        ClipKind::Text => {
            cb.set_text(item.content.clone()).map_err(|e| e.to_string())?;
            *state.last_seen.lock().unwrap() = Some(fingerprint_text(&item.content));
        }
        ClipKind::Image => {
            let file = item.image_file.as_ref().ok_or("图片文件缺失")?;
            let dyn_img = image::open(images_dir().join(file)).map_err(|e| e.to_string())?;
            let rgba = dyn_img.to_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            let data = arboard::ImageData {
                width: w as usize,
                height: h as usize,
                bytes: rgba.into_raw().into(),
            };
            cb.set_image(data).map_err(|e| e.to_string())?;
            if let Some(fp) = &item.image_fp {
                *state.last_seen.lock().unwrap() = Some(fp.clone());
            }
        }
    }

    let mut items = state.items.lock().unwrap();
    if let Some(pos) = items.iter().position(|i| i.id == id) {
        let mut item = items.remove(pos);
        item.created_at = now_str();
        items.insert(0, item);
    }
    save_history(&items)
}

/// 清空历史，并记录当前剪贴板指纹，防止残留内容被监听线程重新写入
pub fn clear_history(state: &ClipboardState) -> Result<(), String> {
    *state.last_seen.lock().unwrap() = current_clipboard_fingerprint();
    let mut items = state.items.lock().unwrap();
    for item in items.iter() {
        remove_image_files(item);
    }
    items.clear();
    save_history(&items)
}

/// 读取当前剪贴板内容的指纹（文本优先，其次图片）
pub fn current_clipboard_fingerprint() -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    if let Ok(text) = cb.get_text() {
        if !text.trim().is_empty() {
            return Some(fingerprint_text(&text));
        }
    }
    if let Ok(img) = cb.get_image() {
        return Some(fingerprint_image(img.width, img.height, &img.bytes));
    }
    None
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

/// 状态栏菜单展示用标签：文本为内容预览，图片为「图片 + 尺寸」
pub fn menu_label(item: &ClipItem) -> String {
    match item.kind {
        ClipKind::Text => preview(&item.content, 40),
        ClipKind::Image => match item.image_size {
            Some((w, h)) => format!("[图片] {}×{}", w, h),
            None => "[图片]".to_string(),
        },
    }
}

/// 读取图片记录的缩略图，作为状态栏菜单项图标
/// （muda 在 macOS 上会自动将图标缩放到 18pt 高度显示）
pub fn load_thumb_image(thumb_file: &str) -> Option<tauri::image::Image<'static>> {
    let path = images_dir().join(thumb_file);
    let img = tauri::image::Image::from_path(path).ok()?;
    Some(img.to_owned())
}

#[tauri::command]
pub fn get_clip_history(state: tauri::State<ClipboardState>) -> Vec<ClipItemView> {
    let items = state.items.lock().unwrap();
    items
        .iter()
        .map(|i| ClipItemView {
            id: i.id.clone(),
            content: i.content.clone(),
            created_at: i.created_at.clone(),
            kind: i.kind,
            image_size: i.image_size,
            thumbnail: match &i.thumb_file {
                Some(f) => thumbnail_data_url(f),
                None => None,
            },
        })
        .collect()
}

/// 获取图片记录的原图 data URL
#[tauri::command]
pub fn get_clip_image(state: tauri::State<ClipboardState>, id: String) -> Option<String> {
    let items = state.items.lock().unwrap();
    let item = items.iter().find(|i| i.id == id && i.kind == ClipKind::Image)?;
    let file = item.image_file.as_ref()?;
    let bytes = std::fs::read(images_dir().join(file)).ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:image/png;base64,{b64}"))
}

#[tauri::command]
pub fn delete_clip_item(state: tauri::State<ClipboardState>, id: String) -> Result<(), String> {
    let mut items = state.items.lock().unwrap();
    if let Some(pos) = items.iter().position(|i| i.id == id) {
        let removed = items.remove(pos);
        remove_image_files(&removed);
    }
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