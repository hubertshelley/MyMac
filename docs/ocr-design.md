# 截图 OCR 功能设计方案

## 一、需求背景

MyMac 已实现「截图与贴图」功能（框选、标注、保存/复制/贴图）。本方案在现有截图流程上新增 **OCR 文字识别**：用户框选屏幕区域后，一键识别其中的文字，并支持复制、保存。

## 二、功能需求

1. **触发入口**
   - 截图覆盖层工具栏新增「识别文字（OCR）」按钮；
   - 框选区域后点击即可识别当前选区内的文字；
   - 识别过程中按钮显示加载态，避免重复点击。

2. **识别结果展示**
   - 识别完成后在覆盖层弹出结果浮层，按行展示识别出的文本；
   - 支持「复制全部」与「复制单行」；
   - 支持关闭浮层回到标注状态继续编辑，或直接复制并结束截图。

3. **识别能力**
   - 支持简体中文与英文混排识别（macOS Vision 原生能力）；
   - 离线识别，不联网、不上传图片，保护隐私；
   - 识别速度快，适合屏幕截图场景。

4. **输出**
   - 复制识别文本到系统剪贴板（不写入粘贴板历史，与 2FA 复制策略一致）；
   - 可选：将识别结果保存为 .txt 文件。

## 三、技术选型

### OCR 引擎：macOS 原生 Vision 框架（首选）

| 方案 | 优点 | 缺点 | 结论 |
| ---- | ---- | ---- | ---- |
| **macOS Vision（VNRecognizeTextRequest）** | 离线、免费、原生、中文识别质量高、速度快、隐私好 | 仅 macOS | ✅ 首选 |
| Tesseract | 跨平台、开源 | 需额外安装/打包语言包、中文识别率一般 | 备选 |
| 云 OCR（百度/腾讯等） | 识别率最高 | 需联网、上传图片、有费用与隐私风险 | 不推荐 |

**结论**：MyMac 是 macOS 原生应用，采用 **Vision 框架**，通过 Rust 的 `objc2-vision` crate 绑定调用，与项目现有 `objc2` / `objc2-foundation` 技术栈一致。

### 依赖确认

- `objc2-vision` 最新版 `0.3.2`，依赖 `objc2 >=0.6.2,<0.8.0`、`objc2-foundation ^0.3.2`，与项目现有 `objc2 0.6` 完全兼容；
- 提供 `VNRecognizeTextRequest`，含 `recognitionLanguages`、`recognitionLevel`、`usesLanguageCorrection`、`results` 等所需 API；
- 需启用 feature：`VNRecognizeTextRequest`、`VNImageRequestHandler`（含 `VNImageRequestHandler` 的 `initWithURL`/`initWithCGImage`）。

## 四、架构与数据流

```
截图覆盖层（Vue）
  │  框选选区 → exportBase64() 得到选区 PNG
  │  invoke("ocr_recognize", { data: base64PNG })
  ▼
Rust 后端 ocr.rs
  │  解码 PNG → 生成 CGImage
  │  VNImageRequestHandler + VNRecognizeTextRequest
  │  （recognitionLevel=accurate, languages=[zh-Hans, en-US]）
  ▼
返回 OcrResult { lines: [{ text, confidence, bbox }] }
  ▼
前端结果浮层：按行展示、复制全部/单行、关闭
```

## 五、后端设计（新增 `src-tauri/src/ocr.rs`）

### 1. 数据结构

```rust
#[derive(Serialize)]
pub struct OcrLine {
    pub text: String,
    pub confidence: f32,
    // 归一化坐标（Vision 坐标系，原点左下），供前端可选高亮
    pub bbox: [f32; 4],
}

#[derive(Serialize)]
pub struct OcrResult {
    pub lines: Vec<OcrLine>,
}
```

### 2. 核心命令

```rust
#[tauri::command]
pub fn ocr_recognize(data: String) -> Result<OcrResult, String> {
    // 1. base64 解码 PNG
    // 2. image crate 解码为 RGBA，构造 CGImage
    // 3. 创建 VNImageRequestHandler
    // 4. 创建 VNRecognizeTextRequest：
    //    - setRecognitionLevel(.accurate)
    //    - setRecognitionLanguages(["zh-Hans", "en-US"])
    //    - setUsesLanguageCorrection(true)
    // 5. handler.perform([request])
    // 6. 遍历 results，提取 text / confidence / boundingBox
}
```

### 3. 关键实现要点

- **CGImage 构造**：用 `image` crate 解码 PNG 为 RGBA8，再通过 `core-graphics` 的 `CGImage` 创建（项目已有 `core-graphics 0.25` 依赖）；
- **同步执行**：`VNImageRequestHandler.perform` 是同步 API，可在后台线程执行，不阻塞主线程；
- **语言**：`recognitionLanguages = ["zh-Hans", "en-US"]`，`usesLanguageCorrection = true` 提升准确率；
- **识别级别**：`recognitionLevel = .accurate`（截图场景对速度不敏感，优先准确率）；
- **坐标**：Vision 返回归一化坐标（原点左下），前端如需高亮需做坐标翻转与缩放换算。

### 4. 注册命令

在 `lib.rs` 的 `invoke_handler` 中新增 `ocr::ocr_recognize`。

## 六、前端设计（修改 `src/views/ScreenshotOverlay.vue`）

### 1. 工具栏新增按钮

在现有工具栏（矩形/箭头/文本/撤销/保存/复制/贴图）中新增「识别文字」按钮，使用 `ScanText` 图标（lucide）。

### 2. 交互流程

```
点击 OCR 按钮
  → 按钮进入 loading 态
  → exportBase64() 获取选区 PNG
  → invoke("ocr_recognize", { data })
  → 成功：显示结果浮层（按行展示文本）
  → 失败：showTip 提示错误
```

### 3. 结果浮层

- 浮层定位在选区附近（复用工具栏定位逻辑）；
- 每行文本可单独点击复制；
- 顶部提供「复制全部」按钮；
- 提供「关闭」按钮（回到标注态）与「复制并完成」按钮（复制后 `finish_screenshot`）。

### 4. 快捷键

- 可考虑 `Cmd+O` 触发 OCR（可选）。

## 七、验收标准

- 框选含中文/英文的区域后点击 OCR，能正确识别并展示文本；
- 复制全部 / 复制单行均能写入系统剪贴板，粘贴内容一致；
- 识别过程不阻塞覆盖层交互，按钮有加载态；
- 关闭浮层可继续标注，复制并完成可正常结束截图；
- 离线可用，不产生网络请求；
- `cargo check` 与前端 `yarn build` 通过。

## 八、实施步骤（建议 TODO 拆分）

1. 更新需求整理文档（docs/requirements.md）与 TODO.md；
2. 后端：新增 `ocr.rs`，实现 Vision 调用与 `ocr_recognize` 命令，注册 handler；
3. 前端：工具栏加 OCR 按钮 + 结果浮层组件；
4. 编译验证（cargo check + yarn build）；
5. 真机验证中文/英文识别与复制。

## 九、风险与备注

- **Vision 版本要求**：`VNRecognizeTextRequest` 需 macOS 10.15+，MyMac 目标系统满足；
- **首次识别延迟**：Vision 首次调用可能稍慢，可接受；
- **坐标换算**：若需在截图上高亮识别区域，需处理 Vision 归一化坐标（左下原点）与前端（左上原点）的翻转；
- **隐私**：全程本地识别，不上传任何图片数据。
