#!/bin/bash
# 本地开发环境启动脚本
# 用法: ./scripts/start-dev.sh [db-only|full]

set -e

MODE=${1:-full}
PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_DIR"

echo "========================================"
echo "Quant Invest 开发环境启动"
echo "模式: $MODE"
echo "========================================"

if ! command -v docker &> /dev/null; then
    echo "错误: Docker 未安装。请访问 https://docs.docker.com/get-docker/ 安装"
    exit 1
fi

if [ "$MODE" = "db-only" ]; then
    echo "启动数据库服务 (PostgreSQL + TimescaleDB + Redis + MinIO)..."
    docker compose -f docker-compose.db-only.yml up -d
    echo ""
    echo "数据库服务已启动:"
    echo "  PostgreSQL:  localhost:5432  (quant_db)"
    echo "  TimescaleDB: localhost:5433  (quant_market)"
    echo "  Redis:       localhost:6379"
    echo "  MinIO:       localhost:9000  (Console: 9001)"
    echo ""
    echo "接下来请手动启动后端:"
    echo "  cd server && python -m uvicorn api.main:app --reload"
    echo "  cd server && celery -A worker.celery_app worker --loglevel=info"

elif [ "$MODE" = "full" ]; then
    echo "启动完整服务栈..."
    docker compose up --build -d
    echo ""
    echo "所有服务已启动:"
    echo "  API:         http://localhost:8000/health"
    echo "  Nginx:       http://localhost"
    echo "  MinIO:       http://localhost:9001"
    echo ""
    echo "查看日志: docker compose logs -f api"
else
    echo "未知模式: $MODE"
    echo "用法: ./scripts/start-dev.sh [db-only|full]"
    exit 1
fi

echo ""
echo "停止命令: docker compose down"
