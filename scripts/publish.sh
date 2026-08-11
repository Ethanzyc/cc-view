#!/usr/bin/env bash
# cc-view 发版脚本：构建双架构 + GitHub/Gitee 双平台发布 + updater latest.json 同步。
#
# 用法：
#   ./scripts/publish.sh 0.5.1   # 发布 v0.5.1
#
# 前置（见 doc/release.md）：
#   - rustup target add aarch64-apple-darwin x86_64-apple-darwin
#   - ~/.tauri/cc-view.key（updater 签名密钥）
#   - gh CLI 已认证（GitHub）
#   - ~/.git-credentials 有 gitee.com 的 token
#   - 版本号已同步改好 Cargo.toml + tauri.conf.json + git push
set -euo pipefail

VERSION="${1:?用法: ./scripts/publish.sh <version> 例: ./scripts/publish.sh 0.5.1}"
TAG="v${VERSION}"
REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_DIR"

AARCH64_DIR="src-tauri/target/aarch64-apple-darwin/release/bundle/macos"
X8664_DIR="src-tauri/target/x86_64-apple-darwin/release/bundle/macos"
GITEE_TOKEN=$(grep 'gitee.com' ~/.git-credentials | head -1 | sed 's|https://[^:]*:\([^@]*\)@.*|\1|')
GITEE_OWNER="Ethanzyc"

if [ -z "$GITEE_TOKEN" ]; then
  echo "ERROR: 无法从 ~/.git-credentials 提取 Gitee token"
  exit 1
fi

echo "=========================================="
echo "  cc-view ${TAG} 双平台发布"
echo "=========================================="

# ── 1. 构建 ──────────────────────────────
echo ""
echo "[1/6] 构建双架构 + 签名..."
export TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/cc-view.key)
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

npm run tauri build -- --target aarch64-apple-darwin --bundles app 2>&1 | tail -5
npm run tauri build -- --target x86_64-apple-darwin --bundles app 2>&1 | tail -5

# ── 2. 打 DMG ───────────────────────────
echo ""
echo "[2/6] 打 DMG..."
./scripts/make-dmg.sh aarch64
./scripts/make-dmg.sh x86_64

# ── 3. 准备 artifacts ───────────────────
echo ""
echo "[3/6] 准备 artifacts..."
STAGING=$(mktemp -d)
cp "${AARCH64_DIR}/cc-view.app.tar.gz" "${STAGING}/cc-view_aarch64.app.tar.gz"
cp "${AARCH64_DIR}/cc-view.app.tar.gz.sig" "${STAGING}/cc-view_aarch64.app.tar.gz.sig"
cp "${X8664_DIR}/cc-view.app.tar.gz" "${STAGING}/cc-view_x86_64.app.tar.gz"
cp "${X8664_DIR}/cc-view.app.tar.gz.sig" "${STAGING}/cc-view_x86_64.app.tar.gz.sig"
SIG_AARCH64=$(cat "${STAGING}/cc-view_aarch64.app.tar.gz.sig")
SIG_X8664=$(cat "${STAGING}/cc-view_x86_64.app.tar.gz.sig")

# ── 4. GitHub release ───────────────────
echo ""
echo "[4/6] 创建 GitHub release ${TAG}..."
RELEASE_NOTES="> ⚠️ 首次打开提示「已损坏」？终端跑：\`xattr -dr com.apple.quarantine /Applications/cc-view.app\`"

if gh release view "$TAG" >/dev/null 2>&1; then
  echo "  GitHub release ${TAG} 已存在，跳过创建"
else
  gh release create "$TAG" --latest \
    --title "${TAG}" \
    --notes "$RELEASE_NOTES"
fi

gh release upload "$TAG" \
  "cc-view_${VERSION}_aarch64.dmg" \
  "cc-view_${VERSION}_x86_64.dmg" \
  "${STAGING}/cc-view_aarch64.app.tar.gz" \
  "${STAGING}/cc-view_aarch64.app.tar.gz.sig" \
  "${STAGING}/cc-view_x86_64.app.tar.gz" \
  "${STAGING}/cc-view_x86_64.app.tar.gz.sig" \
  --clobber

# GitHub latest.json（URL 指向 GitHub）
python3 -c "
import json
data = {
    'version': '${VERSION}',
    'notes': '${TAG}',
    'pub_date': '$(date -u +%Y-%m-%dT%H:%M:%SZ)',
    'platforms': {
        'darwin-aarch64': {
            'signature': open('${STAGING}/cc-view_aarch64.app.tar.gz.sig').read(),
            'url': 'https://github.com/${GITEE_OWNER}/cc-view/releases/download/${TAG}/cc-view_aarch64.app.tar.gz'
        },
        'darwin-x86_64': {
            'signature': open('${STAGING}/cc-view_x86_64.app.tar.gz.sig').read(),
            'url': 'https://github.com/${GITEE_OWNER}/cc-view/releases/download/${TAG}/cc-view_x86_64.app.tar.gz'
        }
    }
}
with open('${STAGING}/github-latest.json', 'w') as f:
    json.dump(data, f, ensure_ascii=False, indent=2)
"
gh release upload "$TAG" "${STAGING}/github-latest.json#latest.json" --clobber

# ── 5. Gitee release ────────────────────
echo ""
echo "[5/6] 创建 Gitee release ${TAG}..."

# 查找/创建 Gitee release
GITEE_RELEASE_ID=$(curl -s "https://gitee.com/api/v5/repos/${GITEE_OWNER}/cc-view/releases?access_token=${GITEE_TOKEN}&per_page=100" \
  | python3 -c "
import sys, json
releases = json.load(sys.stdin)
for r in releases:
    if r['tag_name'] == '${TAG}':
        print(r['id'])
        break
" 2>/dev/null)

if [ -z "$GITEE_RELEASE_ID" ]; then
  GITEE_RELEASE_ID=$(python3 -c "
import json, urllib.request
body = {
    'access_token': '${GITEE_TOKEN}',
    'tag_name': '${TAG}',
    'name': '${TAG}',
    'body': '${RELEASE_NOTES}',
    'prerelease': False,
    'target_commitish': 'main'
}
req = urllib.request.Request(
    'https://gitee.com/api/v5/repos/${GITEE_OWNER}/cc-view/releases',
    data=json.dumps(body).encode('utf-8'),
    headers={'Content-Type': 'application/json'}
)
resp = urllib.request.urlopen(req)
print(json.loads(resp.read())['id'])
")
fi
echo "  Gitee release ID: ${GITEE_RELEASE_ID}"

# 上传 assets 到 Gitee
GITEE_API="https://gitee.com/api/v5/repos/${GITEE_OWNER}/cc-view/releases/${GITEE_RELEASE_ID}/attach_files"
for f in \
  "cc-view_${VERSION}_aarch64.dmg" \
  "cc-view_${VERSION}_x86_64.dmg" \
  "${STAGING}/cc-view_aarch64.app.tar.gz" \
  "${STAGING}/cc-view_aarch64.app.tar.gz.sig" \
  "${STAGING}/cc-view_x86_64.app.tar.gz" \
  "${STAGING}/cc-view_x86_64.app.tar.gz.sig"; do
  BASENAME=$(basename "$f")
  echo "  上传 ${BASENAME}..."
  curl -s -X POST "${GITEE_API}?access_token=${GITEE_TOKEN}" \
    -F "file=@${f}" \
    -F "name=${BASENAME}" >/dev/null
done

# ── 6. 更新 latest.json 到仓库（Gitee raw 兜底用）──
echo ""
echo "[6/6] 更新 latest.json 到仓库（Gitee updater 兜底）..."
python3 -c "
import json
data = {
    'version': '${VERSION}',
    'notes': '${TAG}',
    'pub_date': '$(date -u +%Y-%m-%dT%H:%M:%SZ)',
    'platforms': {
        'darwin-aarch64': {
            'signature': '''${SIG_AARCH64}''',
            'url': 'https://gitee.com/${GITEE_OWNER}/cc-view/releases/download/${TAG}/cc-view_aarch64.app.tar.gz'
        },
        'darwin-x86_64': {
            'signature': '''${SIG_X8664}''',
            'url': 'https://gitee.com/${GITEE_OWNER}/cc-view/releases/download/${TAG}/cc-view_x86_64.app.tar.gz'
        }
    }
}
with open('latest.json', 'w') as f:
    json.dump(data, f, ensure_ascii=False, indent=2)
print('  latest.json 已更新')
"

git add -f latest.json
git commit -m "release: ${TAG} 更新 Gitee updater latest.json" || echo "  无需提交（内容未变）"
git push

# 清理
rm -rf "$STAGING"

echo ""
echo "=========================================="
echo "  ✅ ${TAG} 发布完成！"
echo "  GitHub: https://github.com/${GITEE_OWNER}/cc-view/releases/tag/${TAG}"
echo "  Gitee:  https://gitee.com/${GITEE_OWNER}/cc-view/releases/${TAG}"
echo "=========================================="
