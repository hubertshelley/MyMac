#!/usr/bin/env bash
# 正式打包：release 构建 → build/release/MyMac.app → Developer ID 签名（hardened runtime + 时间戳）
# → 公证（notarytool submit --wait）→ stapler staple → build/release/MyMac-<版本>.zip。
#
# 公证凭据只从钥匙串 profile 读取，不在脚本或仓库里保存任何密码：
#   xcrun notarytool store-credentials "mymac-notary" \
#       --apple-id <Apple ID> --team-id SE3EQ3A77S --password <App 专用密码>
#   NOTARY_PROFILE=mymac-notary bash scripts/release-bundle.sh
# 未设置 NOTARY_PROFILE 时只签名不公证（产物可本机运行，分发给他人会被 Gatekeeper 拦截）。
set -euo pipefail
cd "$(dirname "$0")/.."

BUNDLE_ID="com.hubertshelley.mymac"
VERSION="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import sys,json;print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="mymac"))')"
BUILD_NUMBER="${BUILD_NUMBER:-$(git rev-list --count HEAD 2>/dev/null || echo 1)}"

# 去掉二进制里的本机绝对路径，见 scripts/lib/path-remap.sh
source scripts/lib/path-remap.sh
cargo build --release -p mymac --bin mymac "${PATH_REMAP_ARGS[@]}"

TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])')"
OUT="$PWD/build/release"
APP="$OUT/MyMac.app"
ZIP="$OUT/MyMac-$VERSION.zip"
rm -rf "$APP" "$ZIP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$TARGET_DIR/release/mymac" "$APP/Contents/MacOS/MyMac"
assert_no_home_paths "$APP/Contents/MacOS/MyMac"
cp assets/icon.icns "$APP/Contents/Resources/icon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleName</key><string>MyMac</string>
    <key>CFBundleDisplayName</key><string>MyMac</string>
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

# 正式包不做回退：公证要求 Developer ID + hardened runtime + 安全时间戳，任何一项缺失都直接失败。
IDENTITY="${SIGN_IDENTITY:-$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | sed -E 's/.*"(.*)"/\1/')}"
if [ -z "${IDENTITY:-}" ]; then
    echo "未找到 Developer ID Application 证书，无法正式签名" >&2
    exit 1
fi
if ! codesign --force --timestamp --options runtime -s "$IDENTITY" "$APP"; then
    echo "签名失败：公证要求安全时间戳，请确认能访问 timestamp.apple.com（沙箱 / 离线环境无法正式打包）" >&2
    exit 1
fi
codesign --verify --strict --verbose=2 "$APP"
echo "已签名：$IDENTITY"

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
