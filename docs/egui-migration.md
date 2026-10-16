# MyMac 从 Tauri 迁移到 egui 调整方案

> 编写日期：2026-10-16  
> 源项目：`/Users/hubertshelley/Documents/rust/MyMac`（分支 `main`，最新提交 `9327106 feat: 截图OCR文字识别`，工作区干净）  
> 目标：用 egui / eframe 替换 Tauri 2 + Vue 3 前端，功能对等，保持「轻量、快速、原生」定位。  
> 本文是方案，未修改任何代码；标「待 PoC 验证」的结论需先做技术验证再定稿。

---

## 1. 现状盘点

### 1.1 技术栈与规模

| 层 | 现状 | 规模 |
| --- | --- | --- |
| 桌面框架 | Tauri 2（`tray-icon`、`macos-private-api`、`protocol-asset`） | — |
| 后端 | Rust，`src-tauri/src/` 下 11 个模块 + `screenshot/` 子模块 | 约 4,660 行 |
| 前端 | Vue 3 + TS + Vite + shadcn-vue + Tailwind，`@lucide/vue` 图标 | 约 3,590 行（`.vue/.ts`） |
| Tauri 插件 | opener、dialog、global-shortcut | — |
| 测试 | `totp.rs` 单元测试 3 个；`tests/screenshot_backend.rs` 集成测试 | — |

前端各视图行数（迁移工作量主要在这里）：

| 视图 | 行数 | 说明 |
| --- | --- | --- |
| `ScreenshotOverlay.vue` | 1,267 | 截图覆盖层：冻结背景、窗口高亮、框选、8 控制点、标注、OCR 面板、导出 |
| `BrewView.vue` | 523 | Homebrew 管理：依赖树、进度条、批量更新确认 |
| `TotpView.vue` | 277 | 2FA 列表、倒计时、添加（密钥、链接、截图、剪贴板二维码） |
| `ClipboardView.vue` | 260 | 粘贴板历史：搜索、缩略图、原图查看 |
| `AppsView.vue` | 226 | 应用列表、关联项勾选卸载 |
| `PinView.vue` | 182 | 贴图：拖动、滚轮缩放、右键菜单、双击关闭 |
| `LaunchView.vue` | 157 | 启动项 |
| `DashboardView.vue` | 149 | 资源监控 |
| `SettingsView.vue` | 76 | 状态栏显示配置 |
| `App.vue` + `components/ui/*` | 287 | 侧边栏导航、权限提示、5 个基础组件 |

### 1.2 后端与 Tauri 的耦合点

用 `rg "tauri::|AppHandle|State<|emit\("` 统计，耦合集中在下面几类，**业务逻辑本身基本不依赖 Tauri**：

| 耦合类型 | 位置 | 迁移处理 |
| --- | --- | --- |
| `#[tauri::command]` 和 `tauri::State<T>` 参数 | 全部业务模块，共 47 个命令 | 改成普通函数，参数改为 `&T` 或 `Arc<T>` |
| `app.emit(...)` 事件（9 种） | `brew-progress`、`app-size`、`clip-history-changed`、`navigate-to`、`screenshot-background-ready/failed`、`screenshot-permission-needed`、`totp-code-copied`、`totp-copy-failed` | 统一的 `AppEvent` 通道，加 `ctx.request_repaint()` |
| `tauri::async_runtime::spawn_blocking` | `brew.rs`，约 10 处 | `std::thread::spawn`，结果和进度走通道 |
| `tauri::image::Image` | `status.rs::render_status_icon`、`clipboard.rs::load_thumb_image` | 改为返回 RGBA 和宽高，由 UI 层转成 `tray_icon::Icon` 或 egui 纹理 |
| 托盘和菜单 `tauri::menu` / `tray` | `lib.rs`，约 400 行 | 换成 `tray-icon` + `muda`（Tauri 内部用的就是这两个库，API 很接近） |
| 窗口管理 `WebviewWindowBuilder`、`app.get_webview_window`、`run_on_main_thread`、`ns_window()` | `screenshot/mod.rs` | egui 多视口（viewport）加少量 objc2 调用 |
| `app.path().app_cache_dir()` | `screenshot/mod.rs` | 用 `dirs` crate，**保留原路径** `~/Library/Caches/com.mymac.manager` |
| `tauri-plugin-dialog` 保存对话框 | `save_screenshot_to_file` | `rfd` |
| `tauri-plugin-global-shortcut`（F1） | `lib.rs` | `global-hotkey` |
| `tauri-plugin-opener` | 打开链接、Finder 定位 | `open` crate，或 `Command::new("open")` |
| asset 协议加载截图背景和贴图 | 覆盖层、贴图、粘贴板原图 | **不再需要**：RGBA 直接上传为 egui 纹理 |

直接复用、基本不用改的模块：`system.rs`（sysinfo）、`apps.rs`、`launch.rs`、`config.rs`、`totp.rs`（钥匙串、二维码、otpauth）、`clipboard.rs`（arboard、NSPasteboard changeCount）、`ocr.rs`（Vision）、`screenshot/capture.rs`、`screenshot/winlist.rs`、`status.rs`（fontdue 渲染状态栏图标）。

### 1.3 必须保持不变的约束

1. **Bundle ID `com.mymac.manager`** 和**固定签名证书 `MyMac Codesign`**（见提交 `27a8735`）。屏幕录制授权（TCC）和钥匙串 ACL 都绑定签名身份，任何一项变了，用户就要重新授权，2FA 钥匙串也会反复弹窗。
2. **数据路径不变**：`~/Library/Application Support/MyMac/`（粘贴板历史、配置）、`~/.mymac/totp_accounts.json`、钥匙串里的单一 2FA 密钥库项目、`~/Library/Caches/com.mymac.manager/{screenshots,pins}`。
3. **行为不变**：关闭主窗口时隐藏 Dock 图标、托盘常驻；F1 呼出截图；2 秒刷新状态栏；粘贴板监听沿用现有策略（changeCount 轮询、远程内容延迟 3 秒）。
4. 现有的 `PLAN.md`、`TODO.md`、`docs/requirements.md` 验收标准作为对等验收清单（见第 7 节）。

---

## 2. 迁移收益与代价

**收益**

- 去掉 Node / Vite / Vue / Tailwind 整条前端工具链，项目只剩 Rust，构建和依赖都更简单。
- 截图链路去掉「PNG 编码 → 写临时文件 → asset 协议 → 浏览器解码」这一段，冻结背景直接以 RGBA 纹理上屏，**预计呼出延迟明显下降**（PNG 编码本来就是现有代码注释里写的最耗时一步）。
- 不再用 WebView，空闲内存预计下降（以 PoC 实测为准）。
- 命令和事件从「字符串名 + JSON 序列化」变成直接的 Rust 函数调用和类型化事件，编译期就能发现问题。
- 标注合成改成纯 Rust 离线渲染，可以写单元测试，从根上防止「导出标注偏移」（提交 `b1ef887`）这类问题再出现。

**代价和风险**

- egui 的外观不是原生 macOS 控件，shadcn 风格要用 `egui::Style` 自己调。
- egui 还没到 1.0，版本升级可能有破坏性变更，需要固定版本。
- 中文输入法（IME）、中文字体、多显示器 HiDPI、透明置顶窗口这几项是主要技术风险，见第 6 节。
- 前端约 3,600 行要全部重写，截图覆盖层是重点。

---

## 3. 目标架构

### 3.1 工程结构（Cargo workspace）

```text
MyMac/
├── Cargo.toml                 # [workspace]
├── crates/
│   ├── mymac-core/            # 纯业务逻辑，不依赖任何 UI 框架
│   │   └── src/
│   │       ├── system.rs  apps.rs  launch.rs  brew.rs  config.rs
│   │       ├── clipboard.rs  totp.rs  ocr.rs  status_icon.rs
│   │       ├── screenshot/{capture.rs, winlist.rs, annotate.rs}
│   │       ├── paths.rs       # 统一数据和缓存路径（保持旧路径）
│   │       └── event.rs       # AppEvent 定义 + EventSink trait
│   └── mymac-app/             # eframe 应用
│       ├── src/
│       │   ├── main.rs        # eframe::run_native、激活策略
│       │   ├── app.rs         # MyMacApp: eframe::App，事件分发
│       │   ├── tray.rs        # tray-icon + muda 菜单构建和刷新
│       │   ├── hotkey.rs      # global-hotkey（F1）
│       │   ├── macos.rs       # objc2：Dock 显隐、窗口层级、显示主窗口
│       │   ├── theme.rs       # 字体（中文）、图标字体、Visuals
│       │   ├── widgets/       # Card、Badge、Progress、Switch 等
│       │   ├── views/         # dashboard、apps、launch、brew、clipboard、totp、settings
│       │   ├── screenshot/    # overlay.rs、selection.rs、tools.rs、ocr_panel.rs
│       │   └── pin.rs         # 贴图视口
│       └── assets/            # MaterialIcons-Regular.ttf（复用现有）、图标
└── packaging/                 # 打包配置、Info.plist、entitlements
```

要点：`mymac-core` 不依赖 egui，原来的集成测试 `tests/screenshot_backend.rs` 和 `totp` 单元测试直接搬过来，继续可以用 `cargo test -p mymac-core` 跑。

### 3.2 线程与事件模型

```text
            ┌─────────── 主线程（winit 事件循环 / eframe）───────────┐
 tray 菜单 ─┤ MenuEvent handler ─┐                                    │
 F1 热键  ──┤ GlobalHotKeyEvent ─┼─► AppEvent 通道 ─► MyMacApp::logic │─► 绘制各视口
            │                    │        ▲                           │
            └────────────────────┼────────┼───────────────────────────┘
                                 │        │ send + ctx.request_repaint()
  后台线程：系统监控（2s） ───────┤────────┤
            粘贴板监听（500ms） ──┤────────┤
            Homebrew 任务 ────────┘        │
            截图编码 / OCR / 应用大小 ─────┘
```

- `AppEvent` 是一个枚举，对应现有的 9 种事件，再加上 `TrayAction`、`HotkeyScreenshot`、`BrewTaskDone`、`OcrDone` 等。
- 后台线程持有 `EventSink`（内部是 `crossbeam_channel::Sender<AppEvent>` 加 `egui::Context`），发送后调用 `request_repaint()` 唤醒界面。
- 共享状态 `ClipboardState`、`TotpState`、`SystemMonitor`、`StatusConfig` 继续用 `Arc<Mutex<_>>`，保留现有「先克隆、释放锁、再更新菜单」的防死锁写法。
- 托盘图标和菜单文字在后台线程每 2 秒更新一次。`tray-icon` 和 `muda` 在 macOS 上要求在主线程操作，所以后台线程只发 `AppEvent::StatusTick(snapshot)`，由主线程更新。**这里有前提**：主窗口隐藏时主线程的回调还能正常运行，见 PoC-1。

### 3.3 主窗口隐藏后事件循环的处理（M0 已验证）

**结论：eframe 0.36 原生支持，不需要方案 B（1×1 常驻根视口）。** 0.36 起 `App` 拆成两个方法：

- `logic(&mut self, ctx, frame)`：根视口每次被唤醒都会执行，**窗口隐藏、最小化、被遮挡时也执行**。托盘、热键事件的消费、对子视口发 `ViewportCommand`、`set_dock_visible` 都放这里。
- `ui(&mut self, ui, frame)`：只在窗口可见时执行，只放界面绘制。

后台线程（托盘菜单、热键、监控、粘贴板）经 `mpsc` 发 `AppEvent`，再**调用 `ctx.request_repaint_of(ViewportId::ROOT)`** 唤醒根视口的 `logic`。

踩坑，必须遵守：

1. **不要用 `ctx.request_repaint()` 唤醒**。它等价于 `request_repaint_of(ctx.viewport_id())`，在子视口回调里调用只会重绘子视口，`logic` 只在根视口跑，事件会一直滞留。
2. **子视口的关闭、取消等动作由子视口自己处理**：在回调里直接 `ctx.send_viewport_cmd_to(自己的 id, ...)`，不要绕一圈让根视口处理。截图覆盖层全屏后会遮挡主窗口，根视口此时不一定被调度；PoC 初版就因此出现「覆盖层关不掉」。
3. 子视口创建时固定 `with_visible(false)`，显隐全部用 `ViewportCommand::Visible`；显隐状态用 `Arc<AtomicBool>` 在 `logic` 和子视口回调之间共享。
4. 托盘、热键必须在主线程创建（M0 在 `main()` 里、`run_native` 之前创建，托盘、F1 均正常）。

---

---

## 4. 各模块迁移方案

### 4.1 托盘（`lib.rs` → `tray.rs`）

- 用 `tray-icon` + `muda` 照搬现有菜单结构：CPU、内存、网络（禁用项）、粘贴板历史子菜单、2FA 子菜单、截图（F1）、打开主面板、退出。菜单 ID 规则保持 `clip-item-*`、`totp-item-*`、`clear-clip`、`manage-totp`。
- 状态栏图标：`status::render_status_icon` 改为返回 `(Vec<u8>, w, h)`，UI 层用 `tray_icon::Icon::from_rgba` 生成图标，fontdue 和 MaterialIcons 字体逻辑不变。
- 左键单击显示菜单，对应原来的 `show_menu_on_left_click(true)`：`TrayIconBuilder::with_menu_on_left_click(true)`。
- 托盘要在事件循环启动后创建：放在 `eframe` 的 `AppCreator` 闭包里，此时事件循环已经运行。

### 4.2 全局快捷键（F1）

- `global_hotkey::GlobalHotKeyManager::new()` 必须在主线程创建，同样放在 `AppCreator` 里，注册 `HotKey::new(None, Code::F1)`。
- 用 `GlobalHotKeyEvent::set_event_handler` 处理按键，只在 `Pressed` 时触发，转成 `AppEvent::StartScreenshot`。

### 4.3 主窗口和页面

- 布局：`SidePanel::left` 放 7 个导航项（资源监控、应用卸载、启动项、Homebrew、粘贴板、2FA、设置），`CentralPanel` 显示当前页面。
- 图标：复用仓库里已有的 `src-tauri/fonts/MaterialIcons-Regular.ttf`，作为图标字体加到 `FontDefinitions`，替代 lucide；也可以换成 `egui-phosphor`，二选一。
- **中文字体（必须做）**：egui 默认字体不含中文。启动时优先加载系统 `PingFang.ttc`（指定 `FontData.index`），加载失败时用打包进去的 Noto Sans SC 子集兜底。挂到 Proportional 和 Monospace 两个字体族的回退链上。
- 主题：在 `theme.rs` 集中定义圆角、间距、配色，接近现在的 shadcn 风格；深色和浅色跟随系统（eframe 默认跟随系统，正好覆盖 v0.2 规划里的深色模式）。
- 基础组件：`Card`、`Badge`、`Progress`、`Switch`、`Button` 变体，各写一个小函数或 widget，约 150 行。
- 长列表：`egui_extras::TableBuilder` 或 `ScrollArea::show_rows`，只绘制可见行（粘贴板最多 200 条，应用列表可能上百条）。

各页面要点：

| 页面 | 迁移要点 |
| --- | --- |
| 资源监控 | 直接读 `SystemMonitor::snapshot()`；趋势图用 `egui_plot`（可选，版本需和 egui 对齐） |
| 应用卸载 | 应用大小通过 `AppEvent::AppSize` 增量更新；关联项勾选弹窗用 `egui::Modal`；移入废纸篓沿用 `trash` |
| 启动项 | 搜索过滤、开关、在 Finder 中显示（`open -R`）、删除确认 |
| Homebrew | 后台任务状态机 `Idle / Running{op, stage, current, total} / Done(Result)`；依赖树用 `CollapsingHeader` 按需加载，保留「展开状态不因单项刷新丢失」；更新全部前弹确认框列出过期的顶层软件 |
| 粘贴板 | 缩略图懒加载为 `TextureHandle` 并用 LRU 缓存（上限约 64 张）；查看原图开一个 viewport 或 Modal；收到 `ClipHistoryChanged` 后刷新列表 |
| 2FA | 每秒刷新一次（`request_repaint_after(1s)`），显示倒计时进度；二维码截图沿用 `screencapture -i`，剪贴板识别沿用 `rqrr` |
| 设置 | 状态栏显示项开关，改动后保存配置并立即重绘托盘图标 |
| 权限提示 | 收到 `ScreenshotPermissionNeeded` 后显示横幅，按钮打开系统设置的屏幕录制页 |

### 4.4 截图覆盖层（重点，对应 `ScreenshotOverlay.vue` 的 1,267 行）

**流程对照**

| 步骤 | 现状（Tauri） | egui 方案 |
| --- | --- | --- |
| 抓屏 | 主线程 `CGDisplayCreateImage`，用 `run_on_main_thread` 派发，再轮询等待结果 | 热键回调和 `logic()` 本来就在主线程，**直接调用**，不需要派发和轮询 |
| 背景 | 后台并行编码 PNG → 写文件 → 前端经 asset 协议加载 | BGRA 转 RGBA 后 `ctx.load_texture`，**不编码 PNG、不写文件** |
| 覆盖窗口 | 每个显示器一个 `WebviewWindow`（无边框、透明、置顶、不进任务切换器），再用 objc 设置窗口层级 25 | 每个显示器一个 `show_viewport_immediate`：`ViewportBuilder::with_decorations(false).with_transparent(true).with_position(..).with_inner_size(..).with_window_level(AlwaysOnTop)`，创建后用 objc2 把 NSWindow 层级提高到 25（通过唯一标题在 `NSApp.windows` 里找到对应窗口） |
| 窗口识别 | 前端拿到 `winlist` 后自己做命中判断 | 同样的算法（包含光标且面积最小的窗口优先）移到 Rust，直接复用 `winlist::CaptureWindow` |
| 框选和调整 | Canvas 2D 绘制 | `egui::Painter`：遮罩用 4 个矩形围出选区外区域；选区边框、8 个控制点、尺寸标签；交互用 `ui.interact` 和 `Sense::drag()` |
| 标注 | Canvas 画矩形、箭头、文本 | 屏幕预览用 `Painter`（`rect_stroke`、`line_segment`、`convex_polygon` 画箭头头部、`text`）；文本标注输入用 `TextEdit`（中文输入法见风险） |
| 撤销 | 前端数组 | `Vec<Shape>`，Cmd+Z 或按钮弹出最后一个 |
| 导出 | 前端 Canvas 合成后传 base64 PNG 给后端 | **在 `mymac-core::screenshot::annotate` 里离线合成**：从物理分辨率的原图裁剪，用 `tiny-skia` 画矩形和箭头，用 `fontdue` 或 `ab_glyph` 画文字，坐标乘以显示器缩放比。结果给复制、保存、贴图三条输出共用 |
| OCR | 前端把 PNG 传给后端，Vision 识别后右侧面板展示，支持缩放、多选复制 | 合成后的图直接传给 `ocr::recognize`（改成接收 RGBA 或 PNG 字节，不再用 base64），后台线程执行；右侧 `SidePanel` 展示识别行，支持悬停高亮、多选、复制 |
| 快捷键 | Esc 取消，回车复制 | 在覆盖层视口里读 `ctx.input(|i| i.key_pressed(Key::Escape/Enter))` |
| 保存文件 | dialog 插件 | `rfd::FileDialog::new().set_file_name("截图_YYYYMMDD_HHMMSS.png").save_file()`，在主线程调用 |

标注数据统一用**选区相对的逻辑坐标**存储，导出时统一做 `逻辑 → 物理` 映射，这一层要单元测试覆盖：1x 和 2x 缩放、选区平移、选区缩放后的标注位置。

### 4.5 贴图窗口（对应 `PinView.vue`）

- 每张贴图一个 `show_viewport_deferred`，配置为无边框、置顶，窗口层级同样提到 25。
- 拖动：在主体区域 `drag_started` 时发 `ViewportCommand::StartDrag`。
- 滚轮缩放：读 `smooth_scroll_delta` 计算新尺寸，再发 `ViewportCommand::InnerSize`，保持宽高比。
- 关闭：双击或 Esc，从 `PinState` 里移除，不再渲染这个视口就会自动关闭。
- 右键菜单：`response.context_menu`，提供「复制到粘贴板」「关闭」。
- 贴图数据直接是内存里的 `ColorImage` 和纹理，**不再需要 `pins` 临时目录**；保留启动时清理旧缓存目录的逻辑，兼容老版本残留的文件。

### 4.6 其他替换

| 原能力 | 替换 |
| --- | --- |
| Dock 显隐 `set_dock_visibility` | objc2-app-kit：`NSApplication::setActivationPolicy(Regular / Accessory)` |
| `open_screen_capture_settings` | 不变（本来就是 `open x-apple.systempreferences:...`） |
| opener 插件 | `open` crate |
| 应用缓存路径 | `dirs::cache_dir().join("com.mymac.manager")` |

---

## 5. 依赖清单（版本为 2026-10-16 在 crates.io 查询的最新稳定版）

| crate | 版本 | 用途 |
| --- | --- | --- |
| `eframe` | **=0.36.2**（M0 锁定） | 应用框架，wgpu 后端。0.37.0 改了 `App::update`→`App::ui`，而 egui-phosphor 0.14 仍基于 egui 0.36，待其跟进后再升 |
| `egui-phosphor` | =0.14.0 | 图标（已定） |
| `egui_extras` | 0.37.0 | 表格、图片加载 |
| `egui_plot` | 和 egui 0.37 对齐的版本（待确认） | 资源监控趋势图（可选） |
| `tray-icon` | 0.26.1 | 状态栏图标 |
| `muda` | 0.21.2 | 托盘菜单 |
| `global-hotkey` | 0.8.0 | F1 全局快捷键 |
| `rfd` | 0.17.2 | 保存对话框 |
| `tiny-skia` | 最新 | 标注离线合成（新增） |
| `crossbeam-channel` | 最新 | 事件通道（也可以用 `std::sync::mpsc`） |
| `dirs` / `open` | 最新 | 路径、打开链接 |
| 保留 | — | sysinfo、trash、plist、fontdue、image、rqrr、arboard、chrono、scru128、hmac、sha1、data-encoding、url、keyring、objc2 系列、objc2-vision、core-graphics、core-foundation |
| 移除 | — | tauri、tauri-build、tauri-plugin-*、base64（如果不再需要 base64 传图）；前端整个 `package.json` 和 `node_modules` |

egui 相关的 crate 必须**固定为同一个次版本**（当前 `=0.36.x`），升级时一起升。

---

## 6. 风险与 PoC 验证清单

动手全面重写前，先在一个独立分支里用 2～3 天做下面这些 PoC。每项都给出通过标准和失败时的退路。

| # | 风险点 | 验证方式 | 通过标准 | 失败退路 |
| --- | --- | --- | --- | --- |
| PoC-1 | 主窗口隐藏后，托盘、热键、后台事件是否还能驱动界面和重新显示窗口 | 最小 eframe 程序加 tray-icon 和 global-hotkey；隐藏根视口后，从菜单显示窗口、更新托盘图标文字 | 隐藏 10 分钟后托盘图标仍按 2 秒刷新；菜单和 F1 能把窗口拉回来；Dock 图标随窗口显隐 | 改用方案 B（1×1 常驻根视口），或托盘刷新放到独立的 NSTimer |
| PoC-2 | 多显示器透明置顶覆盖层 | 每个屏幕一个 immediate viewport，铺满屏幕；窗口层级 25；混合 1x 和 2x 屏幕 | 覆盖层盖住菜单栏和 Dock；位置和尺寸准确；鼠标坐标和 CG 全局坐标一致 | 覆盖层窗口直接用 objc2 创建 NSWindow，egui 只负责渲染（工作量大） |
| PoC-3 | 大纹理和呼出延迟 | 5K 和 6K 屏的物理分辨率背景上传为纹理 | 不超过 GPU 最大纹理尺寸（wgpu 常见上限 8192）；从 F1 到覆盖层出现的耗时比现版本短 | 超过上限时分块贴图，或降采样显示、导出时仍用原图 |
| PoC-4 | 中文输入法和字体 | 在 TextEdit 和截图文本标注里用拼音输入法输入中文；加载 PingFang.ttc | 候选框位置正确；输入和删除正常；中文不显示成方块 | 文本标注改为用原生 NSTextField 弹窗输入 |
| PoC-5 | 签名和权限延续 | 用 `MyMac Codesign` 签名、Bundle ID 为 `com.mymac.manager` 的 egui 版本覆盖安装 | 屏幕录制授权和钥匙串访问不需要重新授权 | 检查 entitlements 和签名的 designated requirement，必要时写迁移说明 |
| PoC-6 | 打包 | `cargo-packager` 或 `cargo-bundle` 生成 `.app` 和 `.dmg`，带图标和 Info.plist | 可以双击启动，Gatekeeper 下本机运行正常 | 自己写 xtask 拼装 `.app` 目录，再调用 `codesign` |

---

## 7. 实施阶段

建议在分支 `feature/egui-rewrite` 上推进。**Tauri 版本保持可用直到功能对等**，最后一次性切换。

| 阶段 | 内容 | 产出和验收 | 预估 |
| --- | --- | --- | --- |
| M0 PoC | 第 6 节的 PoC-1～6 | PoC 结论写进 `PLAN.md`，确定方案 A 或 B | 2～3 天 |
| M1 core 抽离 | 建 workspace，把业务模块搬到 `mymac-core`，去掉 Tauri 类型，加上 `AppEvent` 和 `EventSink`；**Tauri 版暂时改为依赖 core，继续能跑** | `cargo check --workspace`、`cargo clippy -- -D warnings`、`cargo test -p mymac-core` 通过；Tauri 版功能不退化 | 2 天 |
| M2 应用骨架 | eframe 主窗口、中文字体和图标字体、主题、侧边栏、托盘、F1、关闭时隐藏 Dock | 托盘菜单完整、2 秒刷新、粘贴板和 2FA 子菜单可复制 | 2～3 天 |
| M3 管理页面 | 资源监控、设置、启动项、应用卸载、粘贴板、2FA | 对照 `docs/requirements.md` 第 1～7 节的验收标准逐条通过 | 4～5 天 |
| M4 Homebrew | 后台任务状态机、进度、依赖树、批量更新 | 对照第 8 节验收标准；任务运行时界面不卡顿 | 2～3 天 |
| M5 截图 | 覆盖层、窗口识别、框选和调整、标注、离线合成、三种输出、OCR | 对照第 9、10 节验收标准；`annotate` 有坐标映射单元测试 | 5～7 天 |
| M6 贴图 | 贴图视口：拖动、缩放、右键菜单、关闭 | 第 9 节贴图相关验收 | 1～2 天 |
| M7 打包切换 | 打包和签名、覆盖安装验证、删除 Tauri 和前端代码、更新 README、PLAN、TODO、requirements 的技术选型 | 覆盖安装后数据和授权都在；`git diff` 里不再有 Vue 和 Tauri 残留 | 1～2 天 |

合计约 **3.5～5 周**（单人）。M5 截图的不确定性最大，PoC-2 和 PoC-4 的结论会直接影响这部分工作量。

### 7.1 对等验收清单（摘自现有 TODO 和需求文档）

- [ ] 资源监控 2 秒刷新；状态栏图标按配置显示 Logo、CPU、内存、磁盘、网络
- [ ] 关闭主窗口时隐藏 Dock 图标，托盘常驻；托盘可以唤起主窗口
- [ ] 应用列表、大小异步计算、关联项勾选、移入废纸篓
- [ ] 启动项：搜索、启用和禁用、在 Finder 中显示、删除用户项
- [ ] 粘贴板：监听（不干扰通用剪贴板）、去重、200 条上限、搜索、删除、清空、点击复制并置顶、查看原图、托盘子菜单
- [ ] 2FA：密钥和链接添加、截图和剪贴板二维码识别、实时验证码和倒计时、复制（不写入粘贴板历史）、托盘子菜单、钥匙串和数据迁移逻辑保留
- [ ] Homebrew：安装引导、换源、搜索、安装、卸载、更新、依赖树、未信任 cask 标注、批量更新确认、进度
- [ ] 截图：F1 和托盘都能触发；主窗口不会被截进去；权限提示；多显示器；窗口高亮和单击选中；拖拽框选；8 个控制点；尺寸提示；矩形、箭头、文本标注；颜色和粗细；撤销；Esc 取消；回车复制；保存文件；贴图
- [ ] OCR：识别、右侧面板、缩放、多选复制
- [ ] 贴图：拖动、滚轮缩放、右键复制、双击或 Esc 关闭；不出现在任务切换器里

### 7.2 每阶段的检查命令

```bash
cargo fmt -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

界面层可以选用 `egui_kittest` 给关键交互写快照或交互测试（例如截图选区调整、标注撤销），不强制。

---

## 8. 需要你决定的事项

1. **PoC 先行**：是否同意先花 2～3 天做 M0，根据结果再决定是否全面迁移？
2. **图标方案**：复用已有的 MaterialIcons 字体，还是换成 `egui-phosphor`（风格更接近 lucide）？
3. **渲染后端**：默认用 wgpu，还是评估 glow（体积更小，但 Apple 已经弃用 OpenGL）？
4. **打包工具**：用 `cargo-packager`（功能全，支持 dmg 和签名），还是自己写 xtask 脚本（可控、依赖少）？
5. **文档同步**：迁移会改变 `PLAN.md` 和 `docs/requirements.md` 里的技术选型和技术方案章节，是否在 M1 开始时同步更新？

---

## 9. M0 技术验证结论（2026-10-16）

PoC 位置：`poc/egui-poc/`（分支 `feature/egui-rewrite`，`src-tauri` 未改动）。构建：`cargo build --release`，再复制进 `MyMacPoC.app` 并 `codesign`。GUI 必须以 `.app` 形式启动，裸二进制在终端沙箱里会静默退出。

| 验证项 | 结果 | 说明 |
| --- | --- | --- |
| wgpu(Metal) 创建窗口 | 通过 | |
| 中文（系统 PingFang）+ phosphor 图标 | 通过 | 截图目测 |
| 关闭主窗口 → 隐藏窗口和 Dock，托盘常驻 | 通过 | `CancelClose` + `NSApplication.setActivationPolicy(Accessory)` |
| 托盘菜单 | 通过 | 真实点击弹出菜单；`menu_on_left_click` 默认就是 true |
| F1 全局快捷键 | 通过 | 真实按键触发；合成按键不触发，**自动化测试不可用，需真人按键** |
| Ctrl+F1 组合 | 通过 | 对照组；F1 无修饰键也可用，**保持 F1** |
| 全屏透明置顶覆盖层 | 通过 | 透明度、置顶、中文标题 |
| 覆盖层按钮 / Esc 关闭 | 通过 | 修复后；日志 `覆盖层自行关闭（按钮/Esc）` |
| F1 开、再按 F1 关 | 通过 | 真实按键 |

M0 初版发现并已修复的 Bug：覆盖层关不掉。根因和规则见 3.3 节。

**未在 M0 覆盖，留到后续阶段**：PoC-3（5K/6K 大纹理）、PoC-4（拼音输入法 IME）、多显示器混合缩放、覆盖层窗口层级 25（目前只用 `always_on_top`）、隐藏 10 分钟后的 2 秒托盘刷新。这些在 M2/M5 开头各做一次小验证。

### 9.1 新约束：Bundle ID 与签名（取代原 PoC-5 的前提）

- **Bundle ID 改为 `com.hubertshelley.mymac`**（原 `com.mymac.manager`）。
- **签名走正式流程**：Developer ID Application 证书 + 公证（类似天工），替代自签 `MyMac Codesign`。xtask 打包需包含 `codesign --options runtime`、`notarytool submit --wait`、`stapler staple`。
- 影响：
  - 屏幕录制授权（TCC）和钥匙串 ACL **首次需重新授权一次**；钥匙串里的 2FA 数据不丢，首次访问点「始终允许」。之后升级由 Team ID 的 designated requirement 保证不再弹窗。
  - 缓存目录 `~/Library/Caches/com.mymac.manager/` 首次启动时搬到 `com.hubertshelley.mymac`。`~/Library/Application Support/MyMac/` 和 `~/.mymac/totp_accounts.json` 不含 Bundle ID，不动。
- PoC-5 的通过标准相应改为：新身份下重新授权一次后，覆盖升级不再弹窗。

### 9.2 M0 结论

**通过，可进入 M1。** 未发现需要放弃 egui 的阻碍；风险集中在 M5 截图（IME、大纹理、多屏），按上面的清单分阶段小验证。

### 9.3 新增截图需求在 egui 下的设计（并入 M5）

**整屏截图**：`winlist` 命中结果为空时，命中「光标所在显示器」，高亮并以 `display` 矩形作为选区。egui 下是 `hit_test(cursor)`：窗口按面积取最小，无窗口则回退到显示器矩形。选区坐标与 CG 全局坐标一致，导出用该显示器的物理分辨率。

**重复触发（覆盖层存在时再次触发）**：
1. 覆盖层是常驻隐藏的 deferred viewport（M0 已验证），重复触发只是「会话替换」，不需要销毁重建窗口。
2. 重新抓屏**必须在覆盖层仍显示时抓取**（`CGDisplayCreateImage` 会包含 `level 25` 的覆盖层窗口，正好得到「带选区与标注的画面」），不能先隐藏覆盖层。这与「首次触发先隐藏主窗口」不同。
3. 顺序：主线程抓新背景 → 新背景编码为纹理 → 重置会话状态（选区、标注、OCR 状态清空）→ 以新背景替换纹理 → 旧会话临时文件清理。抓取完成之前保持旧画面，避免闪烁。
4. 旧选区与标注已被烤进新背景，新会话从空闲态开始。需处理的边界：输入中的文本框先提交或丢弃、OCR 请求中的结果丢弃（用 session id 校验）、保存对话框打开时再触发（忽略，等对话框结束）。
5. 事件走 `AppEvent::StartScreenshot`，`logic` 里判断「有会话 → 替换；无会话 → 新建」。无需跨视口回传，M0 已验证的唤醒方式照用。
