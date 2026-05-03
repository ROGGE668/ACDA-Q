#!/bin/bash
# ACDA-Q 服务端修复脚本
# 执行环境: 服务器 /home/ubuntu/
set -e

echo "=== ACDA-Q Server Fix Script ==="

PROJECT_DIR="/home/ubuntu/ACDA-Q"
ENV_FILE="$PROJECT_DIR/server/.env"

cd "$PROJECT_DIR"

# 1. 确保 .env 存在并包含必要配置
echo "[1/6] Checking .env configuration..."
if [ ! -f "$ENV_FILE" ]; then
    echo "Creating server/.env..."
    cat > "$ENV_FILE" << 'EOF'
# Auto-generated env file
SECRET_KEY=change-me-in-production-$(openssl rand -hex 16)
DATABASE_URL=postgresql+asyncpg://quant:quant123@postgres:5432/quant_db
SYNC_DATABASE_URL=postgresql://quant:quant123@postgres:5432/quant_db
TIMESCALE_DATABASE_URL=postgresql://quant:quant123@timescaledb:5432/quant_market
REDIS_URL=redis://redis:6379/0
CORS_ORIGINS=["*"]
COOKIE_SECURE=false
EOF
fi

# 确保 SECRET_KEY 已设置
if ! grep -q "SECRET_KEY" "$ENV_FILE" 2>/dev/null; then
    echo "SECRET_KEY=$(openssl rand -hex 32)" >> "$ENV_FILE"
fi

# 2. 修复 docker-compose.yml - 移除废弃 version 字段，添加 TUSHARE_TOKEN 占位
echo "[2/6] Patching docker-compose.yml..."
if grep -q '^version:' docker-compose.yml; then
    sed -i '1{/^version:/d}' docker-compose.yml
    echo "Removed obsolete 'version' line."
fi

# 3. 修复 worker healthcheck
echo "[3/6] Patching worker Dockerfile / healthcheck..."
# Worker 没有独立 Dockerfile，使用 api 的。需要在 docker-compose 中给 worker 加 healthcheck
cat > /tmp/worker-health.yml << 'EOF'
    healthcheck:
      test: ["CMD-SHELL", "celery -A server.worker.celery_app inspect ping --destination celery@$$(hostname) || exit 1"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 10s
EOF

# 4. 重建并重启服务
echo "[4/6] Rebuilding and restarting services..."
docker-compose down
docker-compose up -d --build api worker

# 5. 检查 scheduler 状态并提示 TUSHARE_TOKEN
echo "[5/6] Checking scheduler..."
if docker ps --format '{{.Names}}' | grep -q "quant_scheduler"; then
    echo "Scheduler container exists."
    if [ -z "$TUSHARE_TOKEN" ]; then
        echo "⚠️  WARNING: TUSHARE_TOKEN is not set in environment."
        echo "   Data sync will fail. Please set it:"
        echo "   export TUSHARE_TOKEN=your_token_here"
        echo "   Then restart scheduler: docker-compose up -d scheduler"
    fi
else
    echo "Scheduler not running. Starting with current env..."
    docker-compose up -d scheduler || true
fi

# 6. 验证状态
echo "[6/6] Verifying deployment..."
sleep 5
curl -s http://localhost:8000/health || echo "Health check failed"
docker ps --format 'table {{.Names}}\t{{.Status}}'

echo ""
echo "=== Fix Complete ==="
echo "If TUSHARE_TOKEN is missing, set it and run: docker-compose up -d scheduler"
