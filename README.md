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
scripts/bundle.sh     # 打包：默认开发包 build/MyMac.app；release 为签名 + 公证的正式包；install 打包并安装启动
docs/                 # 需求、迁移方案、旧实现参考（docs/legacy-reference）
```

## 开发

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p mymac-core --lib
./scripts/bundle.sh       # 生成 build/MyMac.app（Bundle ID com.hubertshelley.mymac.dev），再用 open 启动
```

GUI 必须以 `.app` 形式启动；直接运行裸二进制在受限终端里会静默退出。迁移进度见 `TODO.md`，方案见 `docs/egui-migration.md`。

## 正式打包与公证

正式 Bundle ID 为 `com.hubertshelley.mymac`，使用 Developer ID Application 证书签名（hardened runtime + 安全时间戳）后提交公证。公证凭据只存在钥匙串 profile 里，仓库不保存密码：

```bash
# 一次性：保存公证凭据（App 专用密码在 appleid.apple.com 生成）
xcrun notarytool store-credentials mymac-notary \
    --apple-id <Apple ID> --team-id SE3EQ3A77S --password <App 专用密码>

# 打包：签名 → 公证 → stapler 装订 → build/release/MyMac-<版本>.zip
NOTARY_PROFILE=mymac-notary ./scripts/bundle.sh release
```

不设置 `NOTARY_PROFILE` 时只签名、不公证。签名需要能访问 Apple 时间戳服务。

本机自用一条命令完成「打正式包 → 退出正在运行的 MyMac → 覆盖安装到 `/Applications` → 启动」：

```bash
./scripts/bundle.sh install                       # 本机自用不需要公证
INSTALL_DIR=~/Applications ./scripts/bundle.sh install   # 可选：换安装目录
```

旧版先收到 SIGTERM，10 秒内未退出会被强制结束；只结束安装目录里的 MyMac，不影响 `build/` 下的开发包。

两种模式都会用 `--remap-path-prefix` 把本机路径（含用户名）从二进制中去掉，并在签名前自检，残留即中止打包。

从旧版（`com.mymac.manager`）升级时：
- 首次启动会自动把 `~/Library/Caches/com.mymac.manager/` 迁移到 `com.hubertshelley.mymac`；粘贴板历史、配置（`~/Library/Application Support/MyMac/`）和 2FA 账户（`~/.mymac/`）路径不变。
- 签名身份变了，需要在「系统设置 → 隐私与安全性 → 屏幕录制」重新授权一次；首次读取 2FA 钥匙串时选「始终允许」。之后升级不再弹窗。
