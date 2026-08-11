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

VERSION="${1:?用法: ./scripts/publish.sh <version> [changelog_file]}"
TAG="v${VERSION}"
NOTES_FILE="${2:-}"
REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_DIR"

GITEE_OWNER="Ethanzyc"
GITEE_TOKEN=$(grep 'gitee.com' ~/.git-credentials | head -1 | sed 's|https://[^:]*:\([^@]*\)@.*|\1|')
if [ -z "$GITEE_TOKEN" ]; then
  echo "ERROR: 无法从 ~/.git-credentials 提取 Gitee token"
  exit 1
fi

AARCH64_DIR="src-tauri/target/aarch64-apple-darwin/release/bundle/macos"
X8664_DIR="src-tauri/target/x86_64-apple-darwin/release/bundle/macos"
STAGING=$(mktemp -d)
trap 'rm -rf "$STAGING"' EXIT

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

# 重签：用 Apple Development 证书替换 ad-hoc，使 DR 跨构建稳定（解决 TCC 更新后失效）
echo "  重签双架构..."
./scripts/resign.sh aarch64-apple-darwin
./scripts/resign.sh x86_64-apple-darwin

# ── 2. 打 DMG ───────────────────────────
echo ""
echo "[2/6] 打 DMG..."
./scripts/make-dmg.sh aarch64
./scripts/make-dmg.sh x86_64

# ── 3. 准备 artifacts（全部重命名到 STAGING）────
echo ""
echo "[3/6] 准备 artifacts..."
cp "${AARCH64_DIR}/cc-view.app.tar.gz"      "${STAGING}/cc-view_aarch64.app.tar.gz"
cp "${AARCH64_DIR}/cc-view.app.tar.gz.sig"  "${STAGING}/cc-view_aarch64.app.tar.gz.sig"
cp "${X8664_DIR}/cc-view.app.tar.gz"        "${STAGING}/cc-view_x86_64.app.tar.gz"
cp "${X8664_DIR}/cc-view.app.tar.gz.sig"    "${STAGING}/cc-view_x86_64.app.tar.gz.sig"
# DMG 已在仓库根目录，名字正确

# ── 4. GitHub release ───────────────────
echo ""
echo "[4/6] 创建 GitHub release ${TAG}..."

if gh release view "$TAG" >/dev/null 2>&1; then
  echo "  GitHub release ${TAG} 已存在，跳过创建"
else
  gh release create "$TAG" --latest \
    --title "${TAG}" \
    --notes "> ⚠️ 首次打开提示「已损坏」？终端跑：\`xattr -dr com.apple.quarantine /Applications/cc-view.app\`"
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
GITHUB_LATEST="${STAGING}/latest.json"
NOTES_ARG=""
if [ -n "$NOTES_FILE" ] && [ -f "$NOTES_FILE" ]; then
  NOTES_ARG="--notes\n$(cat "$NOTES_FILE")"
fi
python3 "$REPO_DIR/scripts/gen-latest-json.py" \
  --version "$VERSION" \
  --aarch64-sig "${STAGING}/cc-view_aarch64.app.tar.gz.sig" \
  --x86-64-sig "${STAGING}/cc-view_x86_64.app.tar.gz.sig" \
  --url-aarch64 "https://github.com/${GITEE_OWNER}/cc-view/releases/download/${TAG}/cc-view_aarch64.app.tar.gz" \
  --url-x86-64 "https://github.com/${GITEE_OWNER}/cc-view/releases/download/${TAG}/cc-view_x86_64.app.tar.gz" \
  --output "$GITHUB_LATEST" \
  ${NOTES_FILE:+--notes "$(cat "$NOTES_FILE")"}
gh release upload "$TAG" "$GITHUB_LATEST" --clobber

# ── 5. Gitee release ────────────────────
echo ""
echo "[5/6] 创建 Gitee release ${TAG}..."

# 查找已有 release
GITEE_RELEASE_ID=$(curl -sf "https://gitee.com/api/v5/repos/${GITEE_OWNER}/cc-view/releases?access_token=${GITEE_TOKEN}&per_page=100" \
  | python3 -c "import sys,json; [print(r['id']) for r in json.load(sys.stdin) if r['tag_name']=='${TAG}']" 2>/dev/null || true)

if [ -z "$GITEE_RELEASE_ID" ]; then
  # 创建 release（用 form-data 避免 JSON 编码问题）
  GITEE_RELEASE_ID=$(curl -sf -X POST "https://gitee.com/api/v5/repos/${GITEE_OWNER}/cc-view/releases" \
    -F "access_token=${GITEE_TOKEN}" \
    -F "tag_name=${TAG}" \
    -F "name=${TAG}" \
    -F "body=cc-view ${TAG}" \
    -F "target_commitish=main" \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
fi
echo "  Gitee release ID: ${GITEE_RELEASE_ID}"

# 上传 assets 到 Gitee（文件名决定 asset 名，所以从 STAGING 拷的带后缀文件直接用）
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
  curl -sf -X POST "${GITEE_API}?access_token=${GITEE_TOKEN}" -F "file=@${f}" >/dev/null
done

# Gitee release latest.json（文件名必须叫 latest.json）
GITEE_LATEST="${STAGING}/latest.json"
python3 "$REPO_DIR/scripts/gen-latest-json.py" \
  --version "$VERSION" \
  --aarch64-sig "${STAGING}/cc-view_aarch64.app.tar.gz.sig" \
  --x86-64-sig "${STAGING}/cc-view_x86_64.app.tar.gz.sig" \
  --url-aarch64 "https://gitee.com/${GITEE_OWNER}/cc-view/releases/download/${TAG}/cc-view_aarch64.app.tar.gz" \
  --url-x86-64 "https://gitee.com/${GITEE_OWNER}/cc-view/releases/download/${TAG}/cc-view_x86_64.app.tar.gz" \
  --output "$GITEE_LATEST" \
  ${NOTES_FILE:+--notes "$(cat "$NOTES_FILE")"}
curl -sf -X POST "${GITEE_API}?access_token=${GITEE_TOKEN}" -F "file=@${GITEE_LATEST}" >/dev/null
echo "  Gitee release latest.json 上传完成"

# ── 6. 更新 latest.json 到仓库（Gitee raw 兜底用）──
echo ""
echo "[6/6] 更新 latest.json 到仓库（Gitee updater 兜底）..."
cp "$GITEE_LATEST" "$REPO_DIR/latest.json"
git add -f latest.json
git commit -m "release: ${TAG} 更新 Gitee updater latest.json" || echo "  无需提交（内容未变）"
git push

echo ""
echo "=========================================="
echo "  ✅ ${TAG} 发布完成！"
echo "  GitHub: https://github.com/${GITEE_OWNER}/cc-view/releases/tag/${TAG}"
echo "  Gitee:  https://gitee.com/${GITEE_OWNER}/cc-view/releases/${TAG}"
echo "=========================================="
