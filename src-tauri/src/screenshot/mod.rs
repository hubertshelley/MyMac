//! 截图与贴图：会话管理、覆盖窗口、贴图窗口与输出命令。
//!
//! 流程：触发（快捷键 / 状态栏）→ 权限检查 → 隐藏主窗口 → 按显示器抓取
//! 冻结背景 → 为每块屏幕创建无边框透明覆盖窗口 → 前端完成框选与标注 →
//! 输出（保存文件 / 复制粘贴板 / 贴图置顶）。

pub mod capture;
pub mod winlist;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, State, WebviewUrl, WebviewWindowBuilder};

/// 覆盖层对应的显示器上下文
#[derive(Debug, Clone, Serialize)]
pub struct OverlayDisplay {
    #[serde(rename = "displayId")]
    pub display_id: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    /// 冻结背景图文件名（前端经 asset 协议加载）
    #[serde(rename = "imageUrl")]
    pub image_url: String,
}

/// 进行中的截图会话
#[derive(Debug)]
pub struct CaptureSession {
    pub id: String,
    pub displays: Vec<OverlayDisplay>,
    pub temp_dir: PathBuf,
}

#[derive(Default)]
pub struct ScreenshotState {
    session: Mutex<Option<CaptureSession>>,
}

/// 贴图元数据
#[derive(Debug, Clone)]
pub struct PinMeta {
    pub file: PathBuf,
    pub width: f64,
    pub height: f64,
}

#[derive(Default)]
pub struct PinState {
    pins: Mutex<HashMap<String, PinMeta>>,
}

/// 在主线程抓取所有显示器画面（CGDisplayCreateImage 必须在主线程调用，
/// 从后台线程调用可能永久挂起）。带 30 秒超时保护。
fn grab_displays_on_main(
    app: &AppHandle,
    displays: &[capture::DisplayInfo],
) -> Result<Vec<(u32, Vec<u8>)>, String> {
    use std::sync::{Arc, Mutex};

    type GrabResult = Arc<Mutex<Option<Result<Vec<(u32, Vec<u8>)>, String>>>>;
    let result: GrabResult = Arc::new(Mutex::new(None));
    let inner = result.clone();
    let displays = displays.to_vec();
    app.run_on_main_thread(move || {
        let mut out = Vec::with_capacity(displays.len());
        for display in &displays {
            match capture::grab_display_png(display.id) {
                Ok((png, _, _)) => out.push((display.id, png)),
                Err(error) => {
                    *inner.lock().unwrap() = Some(Err(error));
                    return;
                }
            }
        }
        *inner.lock().unwrap() = Some(Ok(out));
    })
    .map_err(|e| format!("派发主线程失败：{e}"))?;

    // 轮询等待主线程完成（最长 30 秒）
    for _ in 0..1500 {
        {
            let guard = result.lock().unwrap();
            match guard.as_ref() {
                Some(Ok(data)) => return Ok(data.clone()),
                Some(Err(error)) => return Err(error.clone()),
                None => {}
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err("屏幕抓取超时".to_string())
}

/// 触发一次截图（幂等：已有会话进行中时忽略）
pub fn start_screenshot(app: &AppHandle) -> Result<(), String> {
    {
        let state = app.state::<ScreenshotState>();
        let guard = state.session.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
    }

    if !capture::has_screen_capture_access() {
        // 请求授权（触发系统弹窗），并唤起主窗口提示用户
        capture::request_screen_capture_access();
        show_main_window(app);
        let _ = app.emit("screenshot-permission-needed", ());
        return Err("缺少屏幕录制权限，请在系统设置中允许 MyMac 录制屏幕".to_string());
    }

    run_screenshot_session(app)
}

/// 执行截图会话（不含权限检查）
pub fn run_screenshot_session(app: &AppHandle) -> Result<(), String> {
    // 隐藏主窗口，等待隐藏动画结束再抓屏，避免截入自身
    if let Some(main) = app.get_webview_window("main") {
        if main.is_visible().unwrap_or(false) {
            let _ = main.hide();
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    let displays = capture::active_displays();
    if displays.is_empty() {
        return Err("未找到可用显示器".to_string());
    }

    // 抓取每块屏幕的冻结背景（CGDisplayCreateImage 必须在主线程调用，
    // 否则可能挂起）
    let session_id = scru128::new_string();
    let temp_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("screenshots")
        .join(&session_id);
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("创建临时目录失败：{e}"))?;

    let grabbed = grab_displays_on_main(app, &displays)?;

    let mut overlays = Vec::with_capacity(displays.len());
    for display in &displays {
        let png_bytes = grabbed
            .iter()
            .find(|(id, _)| *id == display.id)
            .map(|(_, png)| png)
            .ok_or_else(|| format!("显示器 {} 抓屏结果缺失", display.id))?;
        let file_name = format!("display_{}.png", display.id);
        std::fs::write(temp_dir.join(&file_name), png_bytes)
            .map_err(|e| format!("写入背景图失败：{e}"))?;
        overlays.push(OverlayDisplay {
            display_id: display.id,
            x: display.x,
            y: display.y,
            width: display.width,
            height: display.height,
            scale: display.scale,
            image_url: file_name,
        });
    }

    // 为每块屏幕创建覆盖窗口
    for (index, overlay) in overlays.iter().enumerate() {
        let label = format!("screenshot-{session_id}-{index}");
        WebviewWindowBuilder::new(
            app,
            &label,
            WebviewUrl::App("index.html?mode=screenshot".into()),
        )
        .title("")
        .position(overlay.x, overlay.y)
        .inner_size(overlay.width, overlay.height)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .shadow(false)
        .focused(index == 0)
        .build()
        .map_err(|e| format!("创建截图覆盖层失败：{e}"))?;
    }

    let state = app.state::<ScreenshotState>();
    *state.session.lock().unwrap() = Some(CaptureSession {
        id: session_id,
        displays: overlays,
        temp_dir,
    });
    Ok(())
}

/// 结束当前截图会话：关闭全部覆盖窗口并清理临时文件
pub fn close_session(app: &AppHandle) {
    let session = {
        let state = app.state::<ScreenshotState>();
        let mut guard = state.session.lock().unwrap();
        guard.take()
    };
    if let Some(session) = session {
        for (_, window) in app.webview_windows() {
            if window.label().starts_with("screenshot-") {
                let _ = window.close();
            }
        }
        let _ = std::fs::remove_dir_all(&session.temp_dir);
    }
}

pub fn show_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_dock_visibility(true);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 清理启动前残留的临时截图/贴图文件
pub fn cleanup_temp_files(app: &AppHandle) {
    if let Ok(cache_dir) = app.path().app_cache_dir() {
        let _ = std::fs::remove_dir_all(cache_dir.join("screenshots"));
        let _ = std::fs::remove_dir_all(cache_dir.join("pins"));
    }
}

// ---------------------------------------------------------------------------
// Tauri 命令
// ---------------------------------------------------------------------------

/// 覆盖层初始化上下文：本屏信息 + 屏幕上的窗口列表
#[derive(Serialize)]
pub struct CaptureContext {
    #[serde(rename = "sessionId")]
    session_id: String,
    display: OverlayDisplay,
    windows: Vec<winlist::CaptureWindow>,
}

#[tauri::command]
pub fn get_capture_context(
    window: tauri::WebviewWindow,
    state: State<'_, ScreenshotState>,
) -> Result<CaptureContext, String> {
    let label = window.label();
    let index: usize = label
        .rsplit('-')
        .next()
        .and_then(|part| part.parse().ok())
        .ok_or_else(|| "无效的覆盖窗口标识".to_string())?;
    let guard = state.session.lock().unwrap();
    let session = guard.as_ref().ok_or_else(|| "截图会话不存在".to_string())?;
    let display = session
        .displays
        .get(index)
        .ok_or_else(|| "覆盖窗口索引越界".to_string())?
        .clone();
    Ok(CaptureContext {
        session_id: session.id.clone(),
        display,
        windows: winlist::list_on_screen_windows(),
    })
}

/// 取消截图（Esc）
#[tauri::command]
pub fn cancel_screenshot(app: AppHandle) {
    close_session(&app);
}

/// 完成输出后关闭会话
#[tauri::command]
pub fn finish_screenshot(app: AppHandle) {
    close_session(&app);
}

pub fn decode_png(data: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|_| "图片数据无效".to_string())
}

/// 复制截图到系统剪贴板
#[tauri::command]
pub fn copy_screenshot_to_clipboard(data: String) -> Result<(), String> {
    let png = decode_png(&data)?;
    let dynamic = image::load_from_memory(&png).map_err(|e| format!("图片解码失败：{e}"))?;
    let rgba = dynamic.to_rgba8();
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: rgba.width() as usize,
            height: rgba.height() as usize,
            bytes: std::borrow::Cow::Owned(rgba.into_raw()),
        })
        .map_err(|e| format!("写入剪贴板失败：{e}"))
}

/// 弹出保存对话框并将截图写入所选路径；用户取消时返回 None
#[tauri::command]
pub fn save_screenshot_to_file(app: AppHandle, data: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let png = decode_png(&data)?;
    let default_name = format!(
        "MyMac截图_{}.png",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let picked = app
        .dialog()
        .file()
        .set_file_name(&default_name)
        .add_filter("PNG 图片", &["png"])
        .blocking_save_file();
    match picked {
        Some(path) => {
            let target: PathBuf = path.into_path().map_err(|e| e.to_string())?;
            std::fs::write(&target, png).map_err(|e| format!("保存文件失败：{e}"))?;
            Ok(Some(target.to_string_lossy().into_owned()))
        }
        None => Ok(None),
    }
}

/// 将截图钉在桌面置顶显示，返回贴图窗口标识
#[tauri::command]
pub fn pin_screenshot(
    app: AppHandle,
    state: State<'_, PinState>,
    data: String,
    width: f64,
    height: f64,
) -> Result<String, String> {
    let png = decode_png(&data)?;
    let pin_id = scru128::new_string();
    let pins_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("pins");
    std::fs::create_dir_all(&pins_dir).map_err(|e| e.to_string())?;
    let file = pins_dir.join(format!("{pin_id}.png"));
    std::fs::write(&file, png).map_err(|e| format!("写入贴图失败：{e}"))?;

    // 初始尺寸限制在合理范围内，保持宽高比
    let max_w = 1100.0_f64.min(width.max(1.0));
    let max_h = 760.0_f64.min(height.max(1.0));
    let shrink = (max_w / width.max(1.0))
        .min(max_h / height.max(1.0))
        .min(1.0);
    let view_w = (width * shrink).round().max(60.0);
    let view_h = (height * shrink).round().max(40.0);

    let label = format!("pin-{pin_id}");
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html?mode=pin".into()))
        .title("")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .inner_size(view_w, view_h)
        .build()
        .map_err(|e| format!("创建贴图窗口失败：{e}"))?;

    state.pins.lock().unwrap().insert(
        pin_id,
        PinMeta {
            file,
            width,
            height,
        },
    );
    Ok(label)
}

/// 贴图窗口初始化上下文
#[derive(Serialize)]
pub struct PinContext {
    #[serde(rename = "imageUrl")]
    image_url: String,
    width: f64,
    height: f64,
}

#[tauri::command]
pub fn get_pin_context(
    window: tauri::WebviewWindow,
    state: State<'_, PinState>,
) -> Result<PinContext, String> {
    let label = window.label();
    let pin_id = label.strip_prefix("pin-").ok_or("无效的贴图窗口标识")?;
    let guard = state.pins.lock().unwrap();
    let meta = guard
        .get(pin_id)
        .ok_or_else(|| "贴图数据不存在".to_string())?;
    Ok(PinContext {
        image_url: meta.file.to_string_lossy().into_owned(),
        width: meta.width,
        height: meta.height,
    })
}

/// 缩放贴图窗口
#[tauri::command]
pub fn resize_pin_window(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    window
        .set_size(LogicalSize::new(width.max(40.0), height.max(30.0)))
        .map_err(|e| e.to_string())
}

/// 关闭贴图窗口并清理文件
#[tauri::command]
pub fn close_pin_window(
    window: tauri::WebviewWindow,
    state: State<'_, PinState>,
) -> Result<(), String> {
    let label = window.label();
    if let Some(pin_id) = label.strip_prefix("pin-") {
        if let Some(meta) = state.pins.lock().unwrap().remove(pin_id) {
            let _ = std::fs::remove_file(meta.file);
        }
    }
    let _ = window.close();
    Ok(())
}

/// 打开系统的屏幕录制权限设置页
#[tauri::command]
pub fn open_screen_capture_settings() -> Result<(), String> {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn()
        .map_err(|e| format!("打开系统设置失败：{e}"))?;
    Ok(())
}
