#!/bin/bash
# ACDA-Q 全新部署脚本
# 在服务器 /home/ubuntu/ACDA-Q 目录执行

set -e

echo "=== ACDA-Q 全新部署 ==="

PROJECT_DIR="/home/ubuntu/ACDA-Q"
cd "$PROJECT_DIR"

# 1. 清理旧容器
echo "[1/5] 清理旧容器..."
docker-compose down --remove-orphans 2>/dev/null || true
docker rm -f quant_api quant_worker quant_scheduler quant_postgres quant_timescaledb quant_redis quant_minio 2>/dev/null || true

# 2. 确保目录结构正确
echo "[2/5] 检查目录结构..."
mkdir -p database/migrations/postgres database/migrations/timescale nginx/ssl

# 3. 写入正确的 Dockerfile
echo "[3/5] 写入 Dockerfile..."
cat > server/Dockerfile << 'EOF'
# 构建阶段
FROM python:3.11-slim as builder

WORKDIR /app

RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update && apt-get install -y --no-install-recommends \
    build-essential libpq-dev \
    && rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
RUN pip install --no-cache-dir --user -r requirements.txt -i https://pypi.tuna.tsinghua.edu.cn/simple

# 运行阶段
FROM python:3.11-slim

WORKDIR /app

RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update && apt-get install -y --no-install-recommends \
    libpq5 curl \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r quant && useradd -r -g quant quant

COPY --from=builder /root/.local /home/quant/.local
ENV PATH=/home/quant/.local/bin:$PATH

COPY --chown=quant:quant . /app/server/

ENV PYTHONPATH=/app
ENV PYTHONDONTWRITEBYTECODE=1
ENV PYTHONUNBUFFERED=1

USER quant

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/health || exit 1

CMD ["uvicorn", "server.api.main:app", "--host", "0.0.0.0", "--port", "8000"]
EOF

# 4. 确保 .env 存在
echo "[4/5] 检查环境变量..."
if [ ! -f "server/.env" ]; then
    cat > server/.env << 'EOF'
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
if ! grep -q "SECRET_KEY" server/.env 2>/dev/null; then
    echo "SECRET_KEY=$(openssl rand -hex 32)" >> server/.env
fi

# 5. 启动服务
echo "[5/5] 启动服务..."
docker-compose up -d

echo ""
echo "=== 部署完成 ==="
echo "等待 15 秒服务启动..."
sleep 15

echo ""
echo "健康检查:"
curl -s http://localhost:8000/health || echo "API 尚未就绪"

echo ""
echo "容器状态:"
docker ps --format "table {{.Names}}\t{{.Status}}"

echo ""
echo "如需配置 TUSHARE_TOKEN，请执行:"
echo "  echo 'TUSHARE_TOKEN=your_token' >> .env"
echo "  docker-compose up -d scheduler"
