#!/bin/bash
# GitHub 仓库初始化脚本
# 用法: ./setup-github.sh <你的GitHub用户名>

set -e

GITHUB_USER="${1:-}"
REPO_NAME="ACDA-Quant"

echo "========================================"
echo "  ACDA-Quant GitHub 设置助手"
echo "========================================"
echo ""

if [ -z "$GITHUB_USER" ]; then
    echo "用法: ./setup-github.sh <你的GitHub用户名>"
    echo "示例: ./setup-github.sh hong"
    exit 1
fi

cd "$(dirname "$0")"

# 检查 git
git status > /dev/null 2>&1 || {
    echo "初始化 git 仓库..."
    git init
    git branch -M main
}

# 配置 git（如果未配置）
if ! git config --global user.email > /dev/null 2>&1; then
    echo "配置 git 用户邮箱..."
    git config user.email "deploy@acda-quant.local"
fi
if ! git config --global user.name > /dev/null 2>&1; then
    echo "配置 git 用户名..."
    git config user.name "ACDA Deploy"
fi

# 检查远程仓库
if git remote | grep -q origin; then
    echo "远程仓库已存在: $(git remote get-url origin)"
else
    echo "添加远程仓库..."
    git remote add origin "git@github.com:${GITHUB_USER}/${REPO_NAME}.git"
    echo "已添加: git@github.com:${GITHUB_USER}/${REPO_NAME}.git"
fi

echo ""
echo "提交代码..."
git add .
git commit -m "feat: 初始提交 + GitHub Actions 自动部署" || echo "没有新更改需要提交"

echo ""
echo "========================================"
echo "  下一步操作"
echo "========================================"
echo ""
echo "1. 在 GitHub 创建仓库:"
echo "   https://github.com/new"
echo "   仓库名: ${REPO_NAME}"
echo "   不要初始化 README"
echo ""
echo "2. 配置 GitHub Secrets:"
echo "   仓库页面 → Settings → Secrets → Actions → New repository secret"
echo ""
echo "   需要添加的 Secrets:"
echo "   • SSH_PRIVATE_KEY    (yuanluezixun.pem 的完整内容)"
echo "   • SERVER_IP          (124.220.70.210)"
echo "   • SERVER_USER        (ubuntu)"
echo ""
echo "3. 推送代码:"
echo "   git push -u origin main"
echo ""
echo "4. 查看部署状态:"
echo "   https://github.com/${GITHUB_USER}/${REPO_NAME}/actions"
echo ""
