#!/bin/bash
set -e

# 把本地 ACDA-Q 代码同步到服务器
# 用法: ./sync-to-server.sh

SERVER_IP="124.220.70.210"
SERVER_USER="ubuntu"
SSH_KEY="${SSH_KEY:-/Users/hong/Documents/yuanluezixun.pem}"
REMOTE_DIR="${REMOTE_DIR:-$HOME/ACDA-Q}"
LOCAL_DIR="/Users/hong/Documents/ACDA-Q"

echo "=== Sync ACDA-Q to Server ==="
echo "Server: $SERVER_USER@$SERVER_IP"
echo "Remote: $REMOTE_DIR"

# 使用 rsync 同步 server 目录（排除不必要的文件）
rsync -avz --delete \
  -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=accept-new" \
  --exclude='__pycache__' \
  --exclude='*.pyc' \
  --exclude='node_modules' \
  --exclude='.DS_Store' \
  --exclude='client/node_modules' \
  --exclude='client/dist' \
  --exclude='client/src-tauri/target' \
  "$LOCAL_DIR/" \
  "$SERVER_USER@$SERVER_IP:$REMOTE_DIR/"

echo ""
echo "=== Sync completed ==="
echo "Next step: SSH to server and run ./deploy.sh"
