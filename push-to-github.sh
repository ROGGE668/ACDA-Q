#!/bin/bash
# 推送 ACDA-Q 到 GitHub
# 用法: ./push-to-github.sh <你的GitHub用户名> <Personal Access Token>

set -e

USER="${1:-ROGGE668}"
TOKEN="${2:-}"

cd "$(dirname "$0")"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  ACDA-Q → GitHub 推送脚本"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

if [ -z "$TOKEN" ]; then
    echo "用法:"
    echo "  ./push-to-github.sh 你的用户名 你的PAT"
    echo ""
    echo "示例:"
    echo "  ./push-to-github.sh ROGGE668 ghp_xxxxxxxx"
    echo ""
    echo "如何获取 PAT:"
    echo "  1. 访问 https://github.com/settings/tokens"
    echo "  2. 点击 Generate new token (classic)"
    echo "  3. Note 填: ACDA-Quant Deploy"
    echo "  4. Expiration 选: No expiration"
    echo "  5. 勾选: repo"
    echo "  6. 点击 Generate token"
    echo "  7. 复制生成的 token"
    exit 1
fi

echo "[1/4] 清理旧的 git lock 文件..."
rm -f .git/HEAD.lock .git/index.lock 2>/dev/null || true

echo "[2/4] 配置远程仓库..."
if git remote | grep -q origin; then
    git remote set-url origin "https://${USER}:${TOKEN}@github.com/${USER}/ACDA-Q.git"
else
    git remote add origin "https://${USER}:${TOKEN}@github.com/${USER}/ACDA-Q.git"
fi

echo "[3/4] 确认提交..."
git add .
git commit -m "feat: 初始提交 + 自动部署配置" || echo "已是最新提交"

# 确保分支是 main
CURRENT=$(git branch --show-current)
if [ "$CURRENT" != "main" ]; then
    git branch -m main 2>/dev/null || true
fi

echo "[4/4] 推送到 GitHub..."
git push -u origin main --force

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  推送成功!"
echo "  仓库: https://github.com/${USER}/ACDA-Q"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
