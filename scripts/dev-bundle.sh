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
    <key>CFBundleName</key><string>MyMac Dev</string>
    <key>CFBundleDisplayName</key><string>MyMac Dev</string>
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

# 用 Developer ID 签名：身份稳定，钥匙串授权一次后跨重建有效。
# 1) --timestamp：可分发标准（需能访问 Apple 时间戳服务器）；
# 2) --timestamp=none：时间戳服务不可达时（如沙箱），身份仍是 Developer ID，
#    本地开发与钥匙串授权不受影响，只是未经公证；
# 3) ad-hoc：最后手段（每次构建身份都变，钥匙串会反复要求授权）。
IDENTITY=$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | sed -E 's/.*"(.*)"/\1/')
if [ -n "${IDENTITY:-}" ]; then
    if codesign --force --timestamp -o runtime -s "$IDENTITY" "$APP" 2>/dev/null; then
        echo "已用 Developer ID 签名（含时间戳）：$IDENTITY"
    elif codesign --force --timestamp=none -o runtime -s "$IDENTITY" "$APP" 2>/dev/null; then
        echo "已用 Developer ID 签名（无时间戳，本地开发用）：$IDENTITY"
    else
        echo "⚠️  Developer ID 签名失败，已回退 ad-hoc。" >&2
        echo "⚠️  ad-hoc 身份每次构建都会变，钥匙串将要求重新授权。" >&2
        codesign --force -s - "$APP"
    fi
else
    echo "⚠️  未找到 Developer ID 证书，已回退 ad-hoc（钥匙串会反复要求授权）。" >&2
    codesign --force -s - "$APP"
fi

# 重建 .app 会让 LaunchServices 丢失注册，导致 open / desktop_app 按 bundle id 找不到
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
"$LSREGISTER" -f "$APP" >/dev/null 2>&1 || true

# 结束正在运行的旧实例：open 对同 bundle id 只会唤出已运行进程，不会启动新二进制。
# 不先杀掉的话，接下来的 `open` 拉起的是旧进程（看不到本次改动，易误判为"改了没用"）。
killall MyMac 2>/dev/null && echo "已结束旧 MyMac 实例；重新 open 将启动本次新构建" || true
echo "$APP"
