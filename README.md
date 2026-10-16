# MyMac

macOS 电脑管家（Rust + egui），菜单栏常驻，提供资源监控、应用卸载、启动项、粘贴板、2FA、Homebrew 管理与截图 / OCR / 贴图。

## 技术栈

- eframe / egui **0.34**（wgpu/Metal），UI 采用 [shadcn-egui](https://crates.io/crates/shadcn-egui) 0.4.0 的 shadcn/ui 设计体系与 Lucide 图标
- 托盘 `tray-icon` + `muda`，全局快捷键 `global-hotkey`（F1 截图）
- 业务内核在 `crates/mymac-core`，不依赖 UI 框架，可独立测试

## 目录

```text
Cargo.toml            # 应用 crate + workspace
src/                  # egui 应用：main / app / theme / tray / hotkey / event / macos
crates/mymac-core/    # 业务内核（system、apps、launch、brew、clipboard、totp、ocr、screenshot 后端）
assets/               # 应用图标
scripts/dev-bundle.sh # 开发打包为 build/MyMac.app（ad-hoc 签名）
poc/egui-poc/         # M0 技术验证记录（不参与构建）
docs/                 # 需求、迁移方案、旧实现参考（docs/legacy-reference）
```

## 开发

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p mymac-core --lib
./scripts/dev-bundle.sh   # 生成 build/MyMac.app，再用 open 启动
```

GUI 必须以 `.app` 形式启动；直接运行裸二进制在受限终端里会静默退出。迁移进度见 `TODO.md`，方案见 `docs/egui-migration.md`。
