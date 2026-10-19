#!/usr/bin/env bash
# MyMac 唯一打包脚本。两种模式共用同一套流程（路径脱敏 → 组装 .app → Developer ID 签名），
# 只在 Bundle ID、编译档位、签名严格程度和是否公证上有区别：
#
#   bash scripts/bundle.sh            # 开发包（默认）：debug 构建 → build/MyMac.app
#                                     #   Bundle ID com.hubertshelley.mymac.dev，显示名 "MyMac Dev"
#                                     #   时间戳服务不可达时退回无时间戳 / ad-hoc 签名，便于本地与沙箱调试
#   bash scripts/bundle.sh release    # 正式包：release 构建 → build/release/MyMac.app + MyMac-<版本>.zip
#                                     #   Bundle ID com.hubertshelley.mymac；签名必须带安全时间戳，失败即中止
#   bash scripts/bundle.sh install    # 本机安装：先完整执行 release，再退出正在运行的 MyMac →
#                                     #   覆盖安装到 /Applications/MyMac.app → 启动
#                                     #   可用 INSTALL_DIR 改安装目录（如 INSTALL_DIR=~/Applications）
#
# 正式包公证凭据只从钥匙串 profile 读取，不在脚本或仓库里保存任何密码：
#   xcrun notarytool store-credentials "mymac-notary" \
#       --apple-id <Apple ID> --team-id SE3EQ3A77S --password <App 专用密码>
#   NOTARY_PROFILE=mymac-notary bash scripts/bundle.sh release
# 未设置 NOTARY_PROFILE 时只签名不公证（产物可本机运行，分发给他人会被 Gatekeeper 拦截）。
#
# 可选环境变量：SIGN_IDENTITY（指定签名证书名）、BUILD_NUMBER（正式包 CFBundleVersion）。
# 产物都在 build/ 下，已 gitignore。GUI 程序必须以 .app 形式启动。
set -euo pipefail
cd "$(dirname "$0")/.."

MODE="${1:-dev}"
case "$MODE" in
    dev)
        BUNDLE_ID="com.hubertshelley.mymac.dev"
        DISPLAY_NAME="MyMac Dev"
        CARGO_PROFILE="debug"
        OUT="$PWD/build"
        ;;
    release|install)
        BUNDLE_ID="com.hubertshelley.mymac"
        DISPLAY_NAME="MyMac"
        CARGO_PROFILE="release"
        OUT="$PWD/build/release"
        ;;
    *)
        echo "用法：bash scripts/bundle.sh [dev|release|install]" >&2
        exit 2
        ;;
esac
# install 的打包部分与 release 完全一致，只在最后多出安装步骤
INSTALL=false
if [ "$MODE" = "install" ]; then
    INSTALL=true
    MODE="release"
    INSTALL_DIR="${INSTALL_DIR:-/Applications}"
    # 打包前先确认能写入，避免编译签名完才发现装不上
    if [ ! -w "$INSTALL_DIR" ]; then
        echo "安装目录不可写：${INSTALL_DIR}（可改用 INSTALL_DIR=~/Applications）" >&2
        exit 1
    fi
fi

metadata() { cargo metadata --no-deps --format-version 1; }
VERSION="$(metadata | python3 -c 'import sys,json;print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="mymac"))')"
TARGET_DIR="$(metadata | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])')"
if [ "$MODE" = "release" ]; then
    BUILD_NUMBER="${BUILD_NUMBER:-$(git rev-list --count HEAD 2>/dev/null || echo 1)}"
else
    BUILD_NUMBER="1"
fi

# ---- 1. 构建（去掉二进制里的本机绝对路径）----
# rustc 会把依赖源码、工具链、构建目录、项目目录的绝对路径编进 panic 位置信息，strip 去不掉，
# 分发出去会暴露用户名和目录结构。用 cargo --config 追加到 target.<host>.rustflags，而不是设置
# RUSTFLAGS：RUSTFLAGS 会整体覆盖 ~/.cargo/config.toml 里的 rustflags（如 lld 链接器参数）。
HOST="$(rustc -vV | sed -n 's/^host: //p')"
REMAP="[\"--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo\", \
\"--remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup\", \
\"--remap-path-prefix=$TARGET_DIR=/target\", \
\"--remap-path-prefix=$PWD=/mymac\"]"
CARGO_ARGS=(build -p mymac --bin mymac --config "target.$HOST.rustflags=$REMAP")
[ "$MODE" = "release" ] && CARGO_ARGS+=(--release)
cargo "${CARGO_ARGS[@]}"

# ---- 2. 组装 .app ----
APP="$OUT/MyMac.app"
ZIP="$OUT/MyMac-$VERSION.zip"
rm -rf "$APP"
[ "$MODE" = "release" ] && rm -f "$ZIP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$TARGET_DIR/$CARGO_PROFILE/mymac" "$APP/Contents/MacOS/MyMac"
cp assets/icon.icns "$APP/Contents/Resources/icon.icns"

# 自检：二进制里仍含本机 home 路径就中止，不产出带用户名的包
LEAKS="$(strings -a "$APP/Contents/MacOS/MyMac" | grep -cF "$HOME/" || true)"
if [ "$LEAKS" != "0" ]; then
    echo "二进制中仍有 $LEAKS 处本机路径（$HOME/...），已中止打包：" >&2
    strings -a "$APP/Contents/MacOS/MyMac" | grep -F "$HOME/" | head -5 >&2
    exit 1
fi
echo "路径自检通过：二进制中无本机 home 路径"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleName</key><string>$DISPLAY_NAME</string>
    <key>CFBundleDisplayName</key><string>$DISPLAY_NAME</string>
    <key>CFBundleExecutable</key><string>MyMac</string>
    <key>CFBundleIconFile</key><string>icon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$BUILD_NUMBER</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

# ---- 3. 签名 ----
# 用 Developer ID 签名：身份稳定，钥匙串 / 屏幕录制授权一次后跨重建有效。
IDENTITY="${SIGN_IDENTITY:-$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | sed -E 's/.*"(.*)"/\1/' || true)}"
if [ "$MODE" = "release" ]; then
    # 正式包不做回退：公证要求 Developer ID + hardened runtime + 安全时间戳，任何一项缺失都直接失败
    if [ -z "$IDENTITY" ]; then
        echo "未找到 Developer ID Application 证书，无法正式签名" >&2
        exit 1
    fi
    if ! codesign --force --timestamp --options runtime -s "$IDENTITY" "$APP"; then
        echo "签名失败：公证要求安全时间戳，请确认能访问 timestamp.apple.com（沙箱 / 离线环境无法正式打包）" >&2
        exit 1
    fi
    codesign --verify --strict --verbose=2 "$APP"
    echo "已签名：$IDENTITY"
elif [ -n "$IDENTITY" ]; then
    # 开发包：时间戳服务不可达（如沙箱）时退回无时间戳，身份仍是 Developer ID，只是不能公证
    if codesign --force --timestamp --options runtime -s "$IDENTITY" "$APP" 2>/dev/null; then
        echo "已用 Developer ID 签名（含时间戳）：$IDENTITY"
    elif codesign --force --timestamp=none --options runtime -s "$IDENTITY" "$APP" 2>/dev/null; then
        echo "已用 Developer ID 签名（无时间戳，本地开发用）：$IDENTITY"
    else
        echo "⚠️  Developer ID 签名失败，已回退 ad-hoc（每次构建身份都变，钥匙串会反复要求授权）。" >&2
        codesign --force -s - "$APP"
    fi
else
    echo "⚠️  未找到 Developer ID 证书，已回退 ad-hoc（钥匙串会反复要求授权）。" >&2
    codesign --force -s - "$APP"
fi

# ---- 4. 收尾 ----
if [ "$MODE" = "release" ]; then
    # 公证上传要 zip；stapler 之后再重新打包，分发的 zip 里才带公证票据
    ditto -c -k --keepParent "$APP" "$ZIP"
    if [ -n "${NOTARY_PROFILE:-}" ]; then
        xcrun notarytool submit "$ZIP" --keychain-profile "$NOTARY_PROFILE" --wait
        xcrun stapler staple "$APP"
        xcrun stapler validate "$APP"
        rm -f "$ZIP"
        ditto -c -k --keepParent "$APP" "$ZIP"
        spctl --assess --type execute --verbose=2 "$APP" || true
        echo "已公证并装订票据"
    else
        echo "⚠️  未设置 NOTARY_PROFILE，跳过公证（仅签名）。" >&2
    fi
    echo "$APP"
    echo "$ZIP"
else
    # 重建 .app 会让 LaunchServices 丢失注册，导致 open 按 bundle id 找不到
    LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
    "$LSREGISTER" -f "$APP" >/dev/null 2>&1 || true
    # open 对同 bundle id 只会唤出已运行进程；先结束旧实例，否则看不到本次改动
    killall MyMac 2>/dev/null && echo "已结束旧 MyMac 实例；重新 open 将启动本次新构建" || true
    echo "$APP"
fi

# ---- 5. 本机安装（仅 install 模式）：退出旧程序 → 覆盖安装 → 启动 ----
if [ "$INSTALL" = true ]; then
    DEST="$INSTALL_DIR/MyMac.app"
    DEST_EXE="$DEST/Contents/MacOS/"

    # 退出正在运行的已安装版本。MyMac 常驻托盘，「Quit」/ AppleScript quit 只会隐藏窗口，
    # 外部只能发信号：先 SIGTERM，最多等 10 秒，仍在就 SIGKILL。
    # 按安装目录内的可执行文件路径匹配（兼容旧版不同的可执行文件名），不影响 build/ 下的开发包。
    PIDS="$(pgrep -f "$DEST_EXE" || true)"
    if [ -n "$PIDS" ]; then
        echo "正在退出旧版 MyMac（pid $(echo $PIDS)）"
        kill $PIDS 2>/dev/null || true
        for _ in $(seq 1 20); do
            pgrep -f "$DEST_EXE" >/dev/null || break
            sleep 0.5
        done
        if pgrep -f "$DEST_EXE" >/dev/null; then
            echo "旧版 10 秒内未退出，强制结束" >&2
            pkill -9 -f "$DEST_EXE" || true
            sleep 1
        fi
    fi

    rm -rf "$DEST"
    ditto "$APP" "$DEST"
    LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
    "$LSREGISTER" -f "$DEST" >/dev/null 2>&1 || true
    echo "已安装到 $DEST"
    open "$DEST"
    echo "已启动 MyMac"
fi
