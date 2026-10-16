#!/usr/bin/env bash
# 开发用：构建并打成 build/MyMac.app（ad-hoc 签名，build/ 已 gitignore）。
# GUI 程序必须以 .app 形式启动，裸二进制在终端沙箱里会静默退出。
# 正式打包（Developer ID 签名 + 公证）见 M7 的 xtask，这里不处理。
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE="${1:-debug}"
if [ "$PROFILE" = "release" ]; then cargo build --release; else cargo build; fi

TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])')"
APP="$PWD/build/MyMac.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$TARGET_DIR/$PROFILE/mymac" "$APP/Contents/MacOS/MyMac"
cp assets/icon.icns "$APP/Contents/Resources/icon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>com.hubertshelley.mymac.dev</string>
    <key>CFBundleName</key><string>MyMac</string>
    <key>CFBundleExecutable</key><string>MyMac</string>
    <key>CFBundleIconFile</key><string>icon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

# 用 Developer ID 签名：身份稳定，钥匙串授权一次后跨重建有效；
# 失败时回退 ad-hoc 并大声警告（ad-hoc 每次构建身份都变，钥匙串会反复弹授权）。
# 注意：Developer ID 必须能访问 Apple 时间戳服务器，沙箱/断网环境会失败。
IDENTITY=$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | sed -E 's/.*"(.*)"/\1/')
if [ -n "${IDENTITY:-}" ]; then
    if codesign --force --timestamp -o runtime -s "$IDENTITY" "$APP" 2>/dev/null; then
        echo "已用 Developer ID 签名：$IDENTITY"
    else
        echo "⚠️  Developer ID 签名失败（无法访问时间戳服务？），已回退 ad-hoc。" >&2
        echo "⚠️  ad-hoc 身份每次构建都会变，钥匙串将要求重新授权；请在联网的本地终端重跑本脚本。" >&2
        codesign --force -s - "$APP"
    fi
else
    echo "⚠️  未找到 Developer ID 证书，已回退 ad-hoc（钥匙串会反复要求授权）。" >&2
    codesign --force -s - "$APP"
fi

# 重建 .app 会让 LaunchServices 丢失注册，导致 open / desktop_app 按 bundle id 找不到
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
"$LSREGISTER" -f "$APP" >/dev/null 2>&1 || true
echo "$APP"
