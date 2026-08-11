#!/usr/bin/env bash
# 重签 cc-view.app：用 Apple Development 证书替换 ad-hoc 签名，
# 使 designated requirement 从 cdhash（每次构建变化）变为 identifier + certificate（跨构建稳定）。
# 解决：Tauri updater 更新后辅助功能权限失效（需删除重加）。
#
# 用法：
#   ./scripts/resign.sh <triple>
#   ./scripts/resign.sh aarch64-apple-darwin
#
# 前置：
#   - Keychain 里有 Apple Development 证书
#   - ~/.tauri/cc-view.key（Tauri updater 签名密钥）
#   - 先 build（npm run tauri build -- --target <triple> --bundles app）
set -euo pipefail

TRIPLE="${1:?用法: ./scripts/resign.sh <triple>}"
REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE_DIR="${REPO_DIR}/src-tauri/target/${TRIPLE}/release/bundle/macos"
APP="${BUNDLE_DIR}/cc-view.app"

if [ ! -d "$APP" ]; then
  echo "ERROR: ${APP} 不存在，请先 build"
  exit 1
fi

# 查找 Apple Development 证书
IDENTITY=$(python3 -c "
import subprocess, re
out = subprocess.check_output(['security', 'find-identity', '-v', '-p', 'codesigning'], text=True)
m = re.search(r'\"(Apple Development[^\"]+)\"', out)
print(m.group(1) if m else '')
")

if [ -z "$IDENTITY" ]; then
  echo "WARNING: 未找到 Apple Development 证书，回退到 ad-hoc 签名"
  echo "         辅助功能权限仍会在每次更新后失效"
  codesign --force --sign - --identifier com.zhuyuchen.cc-view "$APP"
  exit 0
fi

echo "重签 ${APP}"
echo "  证书: ${IDENTITY}"

# 1. 用 Apple Development 证书重签
codesign --force --sign "$IDENTITY" \
  --identifier com.zhuyuchen.cc-view \
  "$APP" 2>&1

# 验证签名
codesign --verify --strict "$APP" 2>&1
echo "  ✅ 签名验证通过"

# 2. 重新创建 updater 包（.app 已变，旧的 .tar.gz 过期）
echo "  重新打包 updater archive..."
rm -f "$BUNDLE_DIR/cc-view.app.tar.gz" "$BUNDLE_DIR/cc-view.app.tar.gz.sig"
tar czf "$BUNDLE_DIR/cc-view.app.tar.gz" -C "$BUNDLE_DIR" cc-view.app

# 3. 用 Tauri signer 重新签名 updater archive（从仓库根目录跑 npx）
echo "  重新签名 updater archive..."
cd "$REPO_DIR"
npx tauri signer sign \
  -f ~/.tauri/cc-view.key \
  -p "" \
  "$BUNDLE_DIR/cc-view.app.tar.gz" 2>&1

echo ""
echo "=== 重签后 DR ==="
codesign -d -r- "$APP" 2>&1 | grep designated
echo ""
echo "✅ 完成：${TRIPLE}"
