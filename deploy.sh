#!/bin/bash
set -e

# ACDA-Quant 服务端部署脚本
# 用法: 把此脚本和项目代码传到服务器后执行

PROJECT_DIR="${PROJECT_DIR:-$HOME/ACDA-Q}"
COMPOSE_FILE="$PROJECT_DIR/docker-compose.prod.yml"

echo "=== ACDA-Quant Server Deploy ==="
echo "Project dir: $PROJECT_DIR"

# 1. 进入项目目录
cd "$PROJECT_DIR" || { echo "目录不存在: $PROJECT_DIR"; exit 1; }

# 2. 拉取最新代码（如果用 Git）
if [ -d .git ]; then
    echo "[1/6] Pulling latest code..."
    git pull origin main
else
    echo "[1/6] 跳过 git pull（非 Git 仓库）"
fi

# 3. 检查环境变量文件
if [ ! -f .env ]; then
    echo "警告: .env 文件不存在，请确认环境变量已配置"
fi

# 4. 构建并重启 API 和 Worker（不重启数据库）
echo "[2/6] Building API and Worker images..."
docker-compose -f "$COMPOSE_FILE" build api worker

echo "[3/6] Restarting API and Worker..."
docker-compose -f "$COMPOSE_FILE" up -d --no-deps api worker

# 5. 运行数据库迁移
echo "[4/6] Running database migrations..."
docker-compose -f "$COMPOSE_FILE" exec -T api alembic upgrade head

# 6. 健康检查
echo "[5/6] Health check..."
sleep 3
if curl -sf http://localhost:8000/health > /dev/null; then
    echo "API health: OK"
else
    echo "API health: FAILED"
fi

# 7. 显示状态
echo "[6/6] Deployment status:"
docker-compose -f "$COMPOSE_FILE" ps api worker

echo ""
echo "=== Deploy completed ==="
