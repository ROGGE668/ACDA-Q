#!/bin/bash
set -e

# ============================================================================
# ACDA-Quant 服务端部署脚本
# 在服务器上执行: cd ~/ACDA-Q && bash server/setup.sh
# ============================================================================

REMOTE_DIR="$HOME/ACDA-Q"
COMPOSE_FILE="docker-compose.prod.yml"

cd "$REMOTE_DIR" || { echo "错误: 目录 $REMOTE_DIR 不存在"; exit 1; }

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║        ACDA-Quant Server Setup                          ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# ---- 检查 Docker ----
if ! command -v docker &> /dev/null; then
    echo "错误: Docker 未安装"
    exit 1
fi

if docker compose version &> /dev/null; then
    DOCKER_COMPOSE="docker compose"
elif command -v docker-compose &> /dev/null; then
    DOCKER_COMPOSE="docker-compose"
else
    echo "错误: Docker Compose 未安装"
    exit 1
fi

# ---- 检查/创建 .env ----
if [ ! -f .env ]; then
    echo "[检查] .env 文件不存在"

    # 尝试从现有容器提取
    EXTRACTED=""
    if docker ps --filter name=quant_api --format '{{.Names}}' | grep -q quant_api; then
        echo "[检查] 发现运行中的 quant_api，尝试提取环境变量..."
        EXTRACTED=$(docker inspect quant_api --format '{{range .Config.Env}}{{.}}{{"\n"}}{{end}}' 2>/dev/null | grep -E '^(DATABASE_URL|SYNC_DATABASE_URL|TIMESCALE_DATABASE_URL|REDIS_URL|SECRET_KEY|POSTGRES)=' || true)
    fi

    if [ -n "$EXTRACTED" ]; then
        echo "$EXTRACTED" > .env
        echo "[OK] 已从现有容器恢复 .env"
    else
        echo ""
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        echo "  需要创建 .env 文件"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        echo ""
        echo "请输入数据库密码（如果不知道，查看之前的部署记录）:"
        read -s -p "Postgres 密码: " DB_PASS
        echo ""

        if [ -z "$DB_PASS" ]; then
            echo "错误: 密码不能为空"
            exit 1
        fi

        SECRET=$(openssl rand -hex 32 2>/dev/null || cat /dev/urandom | tr -dc 'a-zA-Z0-9' | fold -w 64 | head -1)

        cat > .env << EOF
DATABASE_URL=postgresql+asyncpg://quant:${DB_PASS}@postgres:5432/quant_db
SYNC_DATABASE_URL=postgresql://quant:${DB_PASS}@postgres:5432/quant_db
TIMESCALE_DATABASE_URL=postgresql+asyncpg://quant:${DB_PASS}@timescaledb:5432/quant_market
REDIS_URL=redis://redis:6379/0
SECRET_KEY=${SECRET}
CORS_ORIGINS=["*"]
COOKIE_SECURE=false
POSTGRES_USER=quant
POSTGRES_PASSWORD=${DB_PASS}
POSTGRES_DB=quant_db
TIMESCALE_USER=quant
TIMESCALE_PASSWORD=${DB_PASS}
TIMESCALE_DB=quant_market
EOF
        echo "[OK] 已创建 .env 文件"
    fi
else
    echo "[OK] 已找到 .env 文件"
fi

# ---- 确保目录结构 ----
mkdir -p server/reports server/static/admin

# ---- 停止旧容器 ----
echo ""
echo "[1/5] 停止旧容器..."
$DOCKER_COMPOSE -f "$COMPOSE_FILE" stop api worker 2>/dev/null || true

# ---- 构建新镜像 ----
echo "[2/5] 构建 Docker 镜像..."
$DOCKER_COMPOSE -f "$COMPOSE_FILE" build --no-cache api worker

# ---- 启动服务 ----
echo "[3/5] 启动服务..."
$DOCKER_COMPOSE -f "$COMPOSE_FILE" up -d --no-deps api worker

# ---- 等待 API ----
echo "[4/5] 等待 API 就绪..."
for i in $(seq 1 30); do
    if curl -sf http://localhost:8000/health > /dev/null 2>&1; then
        echo "      API 已就绪 ✓"
        break
    fi
    if [ $i -eq 30 ]; then
        echo "      警告: API 启动超时，查看日志:"
        $DOCKER_COMPOSE -f "$COMPOSE_FILE" logs --tail=20 api
        exit 1
    fi
    sleep 1
done

# ---- 数据库迁移 ----
echo "[5/5] 运行数据库迁移..."
if $DOCKER_COMPOSE -f "$COMPOSE_FILE" exec -T api alembic upgrade head; then
    echo "      迁移完成 ✓"
else
    echo "      迁移失败，查看日志:"
    $DOCKER_COMPOSE -f "$COMPOSE_FILE" logs --tail=30 api
    exit 1
fi

# ---- 完成 ----
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  部署成功"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "  API:      http://124.220.70.210:8000"
echo "  Admin:    http://124.220.70.210:8000/admin"
echo "  Health:   http://124.220.70.210:8000/health"
echo ""

$DOCKER_COMPOSE -f "$COMPOSE_FILE" ps api worker 2>/dev/null || true
