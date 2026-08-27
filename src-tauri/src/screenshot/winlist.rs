//! 屏幕窗口枚举：基于 CGWindowList 获取当前屏幕上的普通窗口边界与元数据，
//! 供截图覆盖层做「快速框选应用」的命中匹配。

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::{CFNumber, CFNumberRef};
use core_foundation::string::{CFString, CFStringRef};
use serde::Serialize;

/// 屏幕上一个可被框选的窗口（逻辑坐标，左上原点）
#[derive(Debug, Clone, Serialize)]
pub struct CaptureWindow {
    #[serde(rename = "windowId")]
    pub window_id: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub title: String,
    pub owner: String,
}

const K_WINDOW_LIST_ON_SCREEN_ONLY: u32 = 1 << 0;
const K_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;
const K_NULL_WINDOW_ID: u32 = 0;

extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relativeToWindow: u32) -> CFArrayRef;
}

/// 枚举当前屏幕上的普通图层窗口（layer 0），过滤过小区域
pub fn list_on_screen_windows() -> Vec<CaptureWindow> {
    let mut result = Vec::new();
    unsafe {
        let raw = CGWindowListCopyWindowInfo(
            K_WINDOW_LIST_ON_SCREEN_ONLY | K_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            K_NULL_WINDOW_ID,
        );
        if raw.is_null() {
            return result;
        }
        let array = CFArray::<CFDictionary<CFString, CFType>>::wrap_under_create_rule(raw);
        for entry in array.iter() {
            if let Some(window) = parse_window(&entry) {
                result.push(window);
            }
        }
    }
    result
}

fn parse_window(entry: &CFDictionary<CFString, CFType>) -> Option<CaptureWindow> {
    // 仅普通应用窗口
    let layer = get_number(entry, "kCGWindowLayer")?;
    if layer != 0.0 {
        return None;
    }

    let (x, y, width, height) = get_bounds(entry)?;
    // 过滤过小或无效区域
    if width < 40.0 || height < 40.0 {
        return None;
    }

    let owner = get_string(entry, "kCGWindowOwnerName").unwrap_or_default();
    let title = get_string(entry, "kCGWindowName").unwrap_or_default();

    Some(CaptureWindow {
        window_id: get_number(entry, "kCGWindowNumber").unwrap_or(0.0) as u32,
        x,
        y,
        width,
        height,
        title,
        owner,
    })
}

fn get_bounds(entry: &CFDictionary<CFString, CFType>) -> Option<(f64, f64, f64, f64)> {
    let key = CFString::new("kCGWindowBounds");
    let value = entry.find(key)?;
    let bounds = unsafe {
        CFDictionary::<CFString, CFType>::wrap_under_get_rule(
            value.as_concrete_TypeRef() as CFDictionaryRef
        )
    };
    let x = get_number(&bounds, "X")?;
    let y = get_number(&bounds, "Y")?;
    let width = get_number(&bounds, "Width")?;
    let height = get_number(&bounds, "Height")?;
    Some((x, y, width, height))
}

fn get_number(dict: &CFDictionary<CFString, CFType>, key: &str) -> Option<f64> {
    let key = CFString::new(key);
    let value = dict.find(key)?;
    let number =
        unsafe { CFNumber::wrap_under_get_rule(value.as_concrete_TypeRef() as CFNumberRef) };
    number.to_f64()
}

fn get_string(dict: &CFDictionary<CFString, CFType>, key: &str) -> Option<String> {
    let key = CFString::new(key);
    let value = dict.find(key)?;
    let text = unsafe { CFString::wrap_under_get_rule(value.as_concrete_TypeRef() as CFStringRef) };
    Some(text.to_string())
}
