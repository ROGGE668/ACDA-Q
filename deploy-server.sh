#!/bin/bash
# ACDA-Q 服务器部署脚本
# 在服务器 /home/ubuntu/ACDA-Q 目录执行

set -e

echo "=== ACDA-Q 服务器部署脚本 ==="

PROJECT_DIR="/home/ubuntu/ACDA-Q"
BACKUP_DIR="/home/ubuntu/ACDA-Q-backup-$(date +%Y%m%d_%H%M%S)"

cd "$PROJECT_DIR"

# 1. 备份现有配置
echo "[1/6] 备份现有配置..."
mkdir -p "$BACKUP_DIR"
cp server/.env "$BACKUP_DIR/" 2>/dev/null || true
cp docker-compose.yml "$BACKUP_DIR/" 2>/dev/null || true

# 2. 清理旧容器和卷
echo "[2/6] 清理旧容器..."
docker-compose down --volumes --remove-orphans 2>/dev/null || true

# 3. 清理残留文件
echo "[3/6] 清理残留文件..."
rm -rf server/__pycache__ server/api/__pycache__ server/api/*/__pycache__
rm -rf server/*.pyc server/api/*.pyc server/api/*/*.pyc

# 4. 解压新代码包
echo "[4/6] 解压新代码..."
if [ -f "$PROJECT_DIR/acda-q-server.tar.gz" ]; then
    tar -xzf "$PROJECT_DIR/acda-q-server.tar.gz" -C "$PROJECT_DIR"
    rm "$PROJECT_DIR/acda-q-server.tar.gz"
fi

# 5. 修复 .env 文件
echo "[5/6] 修复配置文件..."
if [ ! -f "server/.env" ] || [ ! -s "server/.env" ]; then
    cat > server/.env << 'EOF'
DATABASE_URL=postgresql+asyncpg://quant:quant123@postgres:5432/quant_db
SYNC_DATABASE_URL=postgresql://quant:quant123@postgres:5432/quant_db
TIMESCALE_DATABASE_URL=postgresql://quant:quant123@timescaledb:5432/quant_market
REDIS_URL=redis://redis:6379/0
CORS_ORIGINS=["*"]
COOKIE_SECURE=false
SECRET_KEY=acda-quant-secret-key-change-me-in-production-2024
EOF
fi

# 6. 构建并启动服务
echo "[6/6] 构建并启动服务..."
docker-compose up -d --build

echo "=== 部署完成 ==="
echo "备份目录: $BACKUP_DIR"
echo ""
echo "检查服务状态:"
sleep 5
docker-compose ps
echo ""
echo "API 健康检查:"
curl -s http://localhost:8000/health || echo "API 未就绪，请稍后再试"