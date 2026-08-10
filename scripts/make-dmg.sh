#!/usr/bin/env bash
# 手动打包 cc-view_<ver>_<arch>.dmg（aarch64 / x86_64）。
# 不走 Tauri bundle_dmg.sh：那个脚本用 AppleScript 控制 Finder 摆图标 / 设背景，
# 在本机会因缺「自动化 → Finder」权限报 -1743。这里直接 hdiutil，纯文件操作。
# staging 放：.app + /Applications 软链 + 「已损坏&首次必读.txt」（Gatekeeper 提示）。
#
# 用法：
#   ./scripts/make-dmg.sh           # 默认 aarch64
#   ./scripts/make-dmg.sh aarch64
#   ./scripts/make-dmg.sh x86_64
#
# 前置：先按对应架构 build（见 doc/release.md）：
#   npm run tauri build -- --target <triple> --bundles app
set -euo pipefail

ARCH="${1:-aarch64}"
case "$ARCH" in
  aarch64) TRIPLE="aarch64-apple-darwin" ;;
  x86_64)  TRIPLE="x86_64-apple-darwin" ;;
  *) echo "用法: $0 [aarch64|x86_64]" >&2; exit 1 ;;
esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/src-tauri/target/$TRIPLE/release/bundle/macos/cc-view.app"

[ -d "$APP" ] || { echo "app not found: ${APP}（先跑 npm run tauri build -- --target $TRIPLE --bundles app）" >&2; exit 1; }

VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$APP/Contents/Info.plist")
OUT="$ROOT/cc-view_${VERSION}_${ARCH}.dmg"
STAGING="$(mktemp -d -t ccview-dmg)"
trap 'rm -rf "$STAGING"' EXIT

cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"

cat > "$STAGING/已损坏&首次必读.txt" <<'EOF'
cc-view 首次打开提示
======================

拖到 Applications 后首次打开若提示「已损坏，无法打开」，
不是真的损坏——是 macOS Gatekeeper 拦截（cc-view 未做 Apple 公证，
个人开源项目无开发者证书）。终端跑一行命令放开即可：

    xattr -dr com.apple.quarantine /Applications/cc-view.app

之后就能正常打开。

首次运行还需在「系统设置 → 隐私与安全性」授权：
  • 通知：系统通知
  • 辅助功能：focus 跳全屏终端（首次点会弹系统授权窗）
EOF

rm -f "$OUT"

hdiutil create \
  -volname "cc-view" \
  -srcfolder "$STAGING" \
  -ov \
  -format UDZO \
  "$OUT" >/dev/null

echo
echo "✓ $OUT"
