//! 截图 OCR：调用 macOS 原生 Vision 框架识别图片中的文字。
//!
//! 流程：前端传入选区 PNG（base64）→ 解码写入临时文件 → 用
//! `VNImageRequestHandler` 加载 → `VNRecognizeTextRequest` 识别 →
//! 返回按行文本（含置信度与归一化坐标）。

use serde::Serialize;

/// 单行识别结果
#[derive(Debug, Clone, Serialize)]
pub struct OcrLine {
    pub text: String,
    pub confidence: f32,
    /// 归一化坐标 [x, y, w, h]，Vision 坐标系（原点左下）
    pub bbox: [f32; 4],
}

/// OCR 识别结果
#[derive(Debug, Clone, Serialize)]
pub struct OcrResult {
    pub lines: Vec<OcrLine>,
}

/// 识别图片中的文字。
/// `data` 为 base64 编码的 PNG 图片数据。
pub fn recognize_png(data: &str) -> Result<OcrResult, String> {
    let png = decode_png(data)?;
    let dynamic = image::load_from_memory(&png).map_err(|e| format!("图片解码失败：{e}"))?;
    let rgba = dynamic.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    if width == 0 || height == 0 {
        return Err("图片尺寸无效".to_string());
    }

    // 将 RGBA 像素写入临时 PNG 文件，供 Vision 通过 URL 加载
    let temp_dir = std::env::temp_dir().join("mymac-ocr");
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("创建临时目录失败：{e}"))?;
    let temp_file = temp_dir.join(format!("{}.png", scru128::new_string()));
    let png_bytes = encode_png(rgba.as_raw(), width, height)?;
    std::fs::write(&temp_file, &png_bytes).map_err(|e| format!("写入临时图片失败：{e}"))?;

    let result = recognize_file(&temp_file);
    let _ = std::fs::remove_file(&temp_file);
    result
}

/// 对指定图片文件执行 Vision 文字识别
fn recognize_file(path: &std::path::Path) -> Result<OcrResult, String> {
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSDictionary, NSString, NSURL};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRequestTextRecognitionLevel,
    };

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));

    // 创建识别请求并配置
    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request.setUsesLanguageCorrection(true);
    request.setAutomaticallyDetectsLanguage(true);
    let languages = NSArray::from_retained_slice(&[
        NSString::from_str("zh-Hans"),
        NSString::from_str("en-US"),
    ]);
    request.setRecognitionLanguages(&languages);

    // 创建图像请求处理器（options 传空字典）
    let options = NSDictionary::new();
    let handler = unsafe {
        VNImageRequestHandler::initWithURL_options(VNImageRequestHandler::alloc(), &url, &options)
    };

    // 同步执行识别。将识别请求作为通用 VNRequest 传入处理器。
    let request_for_handler = request.clone().into_super().into_super();
    let requests = NSArray::from_retained_slice(&[request_for_handler]);
    handler
        .performRequests_error(&requests)
        .map_err(|e| format!("OCR 识别失败：{e}"))?;

    // 提取结果
    let mut lines = Vec::new();
    if let Some(results) = request.results() {
        for observation in results.iter() {
            let candidates = observation.topCandidates(1);
            if candidates.count() > 0 {
                let candidate = candidates.objectAtIndex(0);
                let text = candidate.string().to_string();
                let confidence = candidate.confidence();
                let bbox = unsafe { observation.boundingBox() };
                lines.push(OcrLine {
                    text,
                    confidence,
                    bbox: [
                        bbox.origin.x as f32,
                        bbox.origin.y as f32,
                        bbox.size.width as f32,
                        bbox.size.height as f32,
                    ],
                });
            }
        }
    }

    // Vision 通常按视觉顺序返回；这里保证结果按“从上到下、从左到右”稳定排序。
    lines.sort_by(|a, b| {
        b.bbox[1]
            .partial_cmp(&a.bbox[1])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.bbox[0]
                    .partial_cmp(&b.bbox[0])
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    Ok(OcrResult { lines })
}

/// base64 解码 PNG 数据
fn decode_png(data: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|_| "图片数据无效".to_string())
}

/// 将 RGBA 像素编码为 PNG
fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;

    let mut cursor = std::io::Cursor::new(Vec::new());
    let encoder = PngEncoder::new_with_quality(
        &mut cursor,
        CompressionType::Fast,
        FilterType::NoFilter,
    );
    encoder
        .write_image(
            rgba,
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("PNG 编码失败：{e}"))?;
    Ok(cursor.into_inner())
}

/// Tauri 命令：识别截图选区中的文字。
/// 在后台线程执行，避免阻塞 WebView 异步运行时。
#[tauri::command]
pub async fn ocr_recognize(data: String) -> Result<OcrResult, String> {
    tauri::async_runtime::spawn_blocking(move || recognize_png(&data))
        .await
        .map_err(|e| format!("OCR 后台任务失败：{e}"))?
}

/// Tauri 命令：复制 OCR 文本，并标记为应用主动复制，避免写入粘贴板历史。
#[tauri::command]
pub fn ocr_copy_text(
    clipboard_state: tauri::State<'_, crate::clipboard::ClipboardState>,
    text: String,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("没有可复制的文字".to_string());
    }
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_text(text.clone())
        .map_err(|e| format!("写入剪贴板失败：{e}"))?;
    *clipboard_state.last_seen.lock().unwrap() = Some(text);
    Ok(())
}