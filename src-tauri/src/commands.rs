//! Tauri 命令层：只做参数转换与线程派发，业务逻辑全部在 `mymac-core`。
//! 命令名与参数保持不变，前端 `invoke` 无需改动。

use mymac_core::{
    apps::{self, AppInfo, AppRelatedItem},
    brew::{self, BrewOperationResult, BrewPackage, BrewSource, BrewStatus},
    clipboard::{self, ClipItemView, ClipboardState},
    config::{self, StatusConfig},
    event::{AppEvent, EventSink},
    launch::{self, LaunchItem},
    ocr::{self, OcrResult},
    system::{self, AppState, SystemInfo},
    totp::{self, TotpAccountView, TotpState},
};
use tauri::{AppHandle, Emitter, State};

/// 把 core 事件转成原有的 Tauri 前端事件名。
pub fn event_sink(app: &AppHandle) -> EventSink {
    let app = app.clone();
    EventSink::new(move |event| match event {
        AppEvent::AppSize(info) => {
            let _ = app.emit("app-size", &info);
        }
        AppEvent::BrewProgress(progress) => {
            let _ = app.emit("brew-progress", progress);
        }
    })
}

async fn blocking<T, F>(task: F, what: &str) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("{what}失败：{error}"))
}

// ---- 系统 / 配置 ----

#[tauri::command]
pub fn get_system_info(state: State<AppState>) -> SystemInfo {
    system::get_system_info(&state)
}

#[tauri::command]
pub fn get_status_config(state: State<AppState>) -> StatusConfig {
    config::get_status_config(&state)
}

#[tauri::command]
pub fn set_status_config(state: State<AppState>, config: StatusConfig) -> Result<(), String> {
    config::set_status_config(&state, config)
}

// ---- 应用卸载 ----

#[tauri::command]
pub fn list_apps(app: AppHandle) -> Vec<AppInfo> {
    apps::list_apps(event_sink(&app))
}

#[tauri::command]
pub fn scan_app_related(app: AppInfo) -> Result<Vec<AppRelatedItem>, String> {
    apps::scan_app_related(app)
}

#[tauri::command]
pub fn uninstall_app_items(paths: Vec<String>) -> Result<(), String> {
    apps::uninstall_app_items(paths)
}

// ---- 启动项 ----

#[tauri::command]
pub fn list_launch_items() -> Vec<LaunchItem> {
    launch::list_launch_items()
}

#[tauri::command]
pub fn set_launch_item(path: String, enabled: bool) -> Result<(), String> {
    launch::set_launch_item(path, enabled)
}

#[tauri::command]
pub fn delete_launch_item(path: String) -> Result<String, String> {
    launch::delete_launch_item(path)
}

#[tauri::command]
pub fn reveal_launch_item(path: String) -> Result<(), String> {
    launch::reveal_launch_item(path)
}

// ---- Homebrew ----

#[tauri::command]
pub fn start_brew_install() -> Result<String, String> {
    brew::start_brew_install()
}

#[tauri::command]
pub async fn get_brew_status() -> Result<BrewStatus, String> {
    blocking(brew::get_brew_status, "读取 Homebrew 状态").await
}

#[tauri::command]
pub async fn set_brew_source(app: AppHandle, source: BrewSource) -> Result<String, String> {
    let sink = event_sink(&app);
    blocking(
        move || brew::set_brew_source(&sink, source),
        "切换软件源任务",
    )
    .await?
}

#[tauri::command]
pub async fn list_brew_packages(app: AppHandle) -> Result<Vec<BrewPackage>, String> {
    let sink = event_sink(&app);
    blocking(move || brew::list_brew_packages(&sink), "读取软件列表任务").await?
}

#[tauri::command]
pub async fn get_brew_dependencies(name: String, kind: String) -> Result<Vec<BrewPackage>, String> {
    blocking(
        move || brew::get_brew_dependencies(name, kind),
        "读取依赖任务",
    )
    .await?
}

#[tauri::command]
pub async fn search_brew_packages(query: String) -> Result<Vec<BrewPackage>, String> {
    blocking(move || brew::search_brew_packages(query), "搜索软件任务").await?
}

#[tauri::command]
pub async fn install_brew_package(
    app: AppHandle,
    name: String,
    kind: String,
) -> Result<BrewOperationResult, String> {
    let sink = event_sink(&app);
    blocking(
        move || brew::install_brew_package(&sink, name, kind),
        "Homebrew 后台任务",
    )
    .await?
}

#[tauri::command]
pub async fn uninstall_brew_package(
    app: AppHandle,
    name: String,
    kind: String,
) -> Result<BrewOperationResult, String> {
    let sink = event_sink(&app);
    blocking(
        move || brew::uninstall_brew_package(&sink, name, kind),
        "Homebrew 后台任务",
    )
    .await?
}

#[tauri::command]
pub async fn refresh_brew_package(name: String, kind: String) -> Result<BrewPackage, String> {
    blocking(
        move || brew::refresh_brew_package(name, kind),
        "刷新软件状态任务",
    )
    .await?
}

#[tauri::command]
pub async fn upgrade_brew_package(
    app: AppHandle,
    name: String,
    kind: String,
) -> Result<BrewOperationResult, String> {
    let sink = event_sink(&app);
    blocking(
        move || brew::upgrade_brew_package(&sink, name, kind),
        "Homebrew 后台任务",
    )
    .await?
}

#[tauri::command]
pub async fn upgrade_all_brew_packages(app: AppHandle) -> Result<BrewOperationResult, String> {
    let sink = event_sink(&app);
    blocking(
        move || brew::upgrade_all_brew_packages(&sink),
        "批量更新后台任务",
    )
    .await?
}

// ---- 粘贴板 ----

#[tauri::command]
pub fn get_clip_history(state: State<ClipboardState>) -> Vec<ClipItemView> {
    clipboard::get_clip_history(&state)
}

#[tauri::command]
pub fn get_clip_image(state: State<ClipboardState>, id: String) -> Option<String> {
    clipboard::get_clip_image(&state, id)
}

#[tauri::command]
pub fn delete_clip_item(state: State<ClipboardState>, id: String) -> Result<(), String> {
    clipboard::delete_clip_item(&state, id)
}

#[tauri::command]
pub fn clear_clip_history(state: State<ClipboardState>) -> Result<(), String> {
    clipboard::clear_clip_history(&state)
}

#[tauri::command]
pub fn copy_clip_item(state: State<ClipboardState>, id: String) -> Result<(), String> {
    clipboard::copy_clip_item(&state, id)
}

// ---- 2FA ----

#[tauri::command]
pub fn get_totp_accounts(state: State<TotpState>) -> Result<Vec<TotpAccountView>, String> {
    totp::get_totp_accounts(&state)
}

#[tauri::command]
pub fn add_totp_account(
    state: State<TotpState>,
    name: String,
    issuer: String,
    secret: String,
    digits: u32,
    period: u64,
) -> Result<TotpAccountView, String> {
    totp::add_totp_account(&state, name, issuer, secret, digits, period)
}

#[tauri::command]
pub fn delete_totp_account(state: State<TotpState>, id: String) -> Result<(), String> {
    totp::delete_totp_account(&state, id)
}

#[tauri::command]
pub fn copy_totp_code(
    totp_state: State<TotpState>,
    clipboard_state: State<ClipboardState>,
    id: String,
) -> Result<String, String> {
    totp::copy_totp_code(&totp_state, &clipboard_state, id)
}

#[tauri::command]
pub fn decode_totp_qr_image(data: String) -> Result<String, String> {
    totp::decode_totp_qr_image(data)
}

#[tauri::command]
pub fn capture_totp_qr() -> Result<String, String> {
    totp::capture_totp_qr()
}

#[tauri::command]
pub fn decode_totp_qr_clipboard() -> Result<String, String> {
    totp::decode_totp_qr_clipboard()
}

// ---- OCR ----

#[tauri::command]
pub async fn ocr_recognize(data: String) -> Result<OcrResult, String> {
    blocking(move || ocr::recognize_png(&data), "OCR 后台任务").await?
}

#[tauri::command]
pub fn ocr_copy_text(state: State<ClipboardState>, text: String) -> Result<(), String> {
    ocr::ocr_copy_text(&state, text)
}
