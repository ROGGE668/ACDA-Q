#!/bin/bash
set -e

# ============================================================================
# ACDA-Quant 服务器端一键部署脚本
# 用法: ./deploy-server.sh [SSH密钥路径]
# ============================================================================

# ---- 基础配置 ----
SERVER_IP="124.220.70.210"
SERVER_USER="ubuntu"
REMOTE_DIR="~/ACDA-Q"
LOCAL_DIR="/Users/hong/Documents/ACDA-Q"
COMPOSE_FILE="docker-compose.prod.yml"

# ---- 颜色输出 ----
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

info()    { echo -e "${BLUE}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[OK]${NC} $1"; }
warn()    { echo -e "${YELLOW}[WARN]${NC} $1"; }
error()   { echo -e "${RED}[ERR]${NC} $1"; }
step()    { echo -e "\n${CYAN}━━━ $1 ━━━${NC}"; }

# ---- SSH 密钥查找 ----
find_ssh_key() {
    if [ -n "$1" ]; then
        echo "$1"
        return
    fi
    if [ -n "$SSH_KEY" ] && [ -f "$SSH_KEY" ]; then
        echo "$SSH_KEY"
        return
    fi
    for path in \
        "$HOME/Documents/ACDA-Q/yuanluezixun.pem" \
        "$HOME/Documents/yuanluezixun.pem" \
        "$HOME/.ssh/yuanluezixun.pem" \
        "$HOME/Downloads/yuanluezixun.pem" \
        "$HOME/yuanluezixun.pem"; do
        if [ -f "$path" ]; then
            echo "$path"
            return
        fi
    done
}

# ---- SSH 执行命令 ----
ssh_run() {
    ssh -i "$SSH_KEY" -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 "$SERVER_USER@$SERVER_IP" "$@"
}

# ---- 主流程 ----
clear
echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║        ACDA-Quant 服务端一键部署脚本 v2.0               ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# 1. 查找密钥
info "查找 SSH 密钥..."
SSH_KEY=$(find_ssh_key "$1")
if [ -z "$SSH_KEY" ] || [ ! -f "$SSH_KEY" ]; then
    error "SSH 密钥未找到"
    echo ""
    echo "用法: ./deploy-server.sh [密钥路径]"
    echo ""
    echo "示例:"
    echo "  ./deploy-server.sh ~/Documents/yuanluezixun.pem"
    echo "  SSH_KEY=~/.ssh/id_rsa ./deploy-server.sh"
    exit 1
fi
chmod 600 "$SSH_KEY"
success "使用密钥: $SSH_KEY"

# 2. 测试 SSH
info "测试 SSH 连接..."
if ! ssh_run "echo 'SSH_OK'" > /dev/null 2>&1; then
    error "无法连接到 $SERVER_IP"
    exit 1
fi
success "SSH 连接正常"

# 3. 检查服务器环境
step "检查服务器环境"

# 检查服务器上项目目录是否存在
if ! ssh_run "test -d $REMOTE_DIR" > /dev/null 2>&1; then
    warn "服务器目录 $REMOTE_DIR 不存在，尝试创建..."
    ssh_run "mkdir -p $REMOTE_DIR" || {
        error "无法创建远程目录"
        exit 1
    }
    success "已创建远程目录"
fi

# 检查 .env 文件
ENV_EXISTS=$(ssh_run "test -f $REMOTE_DIR/.env && echo 'YES' || echo 'NO'" 2>/dev/null)
HAS_RUNNING=$(ssh_run "docker ps --filter name=quant_api --format '{{.Names}}' 2>/dev/null | head -1" 2>/dev/null || echo "")

if [ "$ENV_EXISTS" = "YES" ]; then
    success "已找到 .env 文件"
elif [ -n "$HAS_RUNNING" ]; then
    warn ".env 文件缺失，但检测到已有运行中的容器"
    info "正在从现有容器提取环境变量..."
    ssh_run "docker inspect quant_api --format '{{range .Config.Env}}{{.}}\n{{end}}' > /tmp/extracted_env.txt 2>/dev/null || true"
    ENV_COUNT=$(ssh_run "wc -l < /tmp/extracted_env.txt 2>/dev/null || echo 0")
    if [ "$ENV_COUNT" -gt 3 ]; then
        ssh_run "cp /tmp/extracted_env.txt $REMOTE_DIR/.env"
        success "已从现有容器恢复 .env 文件"
        warn "建议检查 $REMOTE_DIR/.env 是否正确"
    else
        error "无法从现有容器提取配置"
        ENV_EXISTS="NO"
    fi
else
    ENV_EXISTS="NO"
fi

# 如果仍然没有 .env，生成模板并退出
if [ "$ENV_EXISTS" != "YES" ]; then
    error ".env 文件不存在，部署无法继续"
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  需要在服务器上创建 .env 文件"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "请执行以下命令（在本地终端）:"
    echo ""
    echo "  ssh -i $SSH_KEY $SERVER_USER@$SERVER_IP"
    echo "  cd ~/ACDA-Q"
    echo "  cat > .env << 'EOF'"
    echo "DATABASE_URL=postgresql+asyncpg://quant:你的密码@postgres:5432/quant_db"
    echo "SYNC_DATABASE_URL=postgresql://quant:你的密码@postgres:5432/quant_db"
    echo "TIMESCALE_DATABASE_URL=postgresql+asyncpg://quant:你的密码@timescaledb:5432/quant_market"
    echo "REDIS_URL=redis://redis:6379/0"
    echo "SECRET_KEY=$(openssl rand -hex 32 2>/dev/null || echo '请手动设置32位以上随机字符串')"
    echo "CORS_ORIGINS=[\"*\"]"
    echo "COOKIE_SECURE=false"
    echo "POSTGRES_USER=quant"
    echo "POSTGRES_PASSWORD=你的密码"
    echo "POSTGRES_DB=quant_db"
    echo "TIMESCALE_USER=quant"
    echo "TIMESCALE_PASSWORD=你的密码"
    echo "TIMESCALE_DB=quant_market"
    echo "EOF"
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    exit 1
fi

# 4. 同步代码
step "同步代码到服务器"

info "同步 server/ 目录..."
rsync -avz --delete \
    -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=accept-new" \
    --exclude='__pycache__' \
    --exclude='*.pyc' \
    --exclude='.pytest_cache' \
    --exclude='target' \
    "$LOCAL_DIR/server/" \
    "$SERVER_USER@$SERVER_IP:$REMOTE_DIR/server/"

info "同步 docker-compose.prod.yml..."
rsync -avz \
    -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=accept-new" \
    "$LOCAL_DIR/$COMPOSE_FILE" \
    "$SERVER_USER@$SERVER_IP:$REMOTE_DIR/$COMPOSE_FILE"

success "代码同步完成"

# 5. 服务器端部署
step "服务器端部署"

ssh -i "$SSH_KEY" -o StrictHostKeyChecking=accept-new "$SERVER_USER@$SERVER_IP" bash -s << 'REMOTE_SCRIPT'
    set -e

    REMOTE_DIR="$HOME/ACDA-Q"
    COMPOSE_FILE="docker-compose.prod.yml"

    cd "$REMOTE_DIR" || { echo "目录不存在"; exit 1; }

    # 检查 Docker
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

    echo ""
    echo "[1/5] 停止旧容器 (api, worker)..."
    $DOCKER_COMPOSE -f "$COMPOSE_FILE" stop api worker 2>/dev/null || true

    echo "[2/5] 构建 Docker 镜像..."
    $DOCKER_COMPOSE -f "$COMPOSE_FILE" build --no-cache api worker

    echo "[3/5] 启动新容器..."
    $DOCKER_COMPOSE -f "$COMPOSE_FILE" up -d --no-deps api worker

    echo "[4/5] 等待 API 就绪..."
    for i in $(seq 1 30); do
        if curl -sf http://localhost:8000/health > /dev/null 2>&1; then
            echo "      API 已就绪 ✓"
            break
        fi
        if [ $i -eq 30 ]; then
            echo "      警告: API 启动超时"
        fi
        sleep 1
    done

    echo "[5/5] 运行数据库迁移..."
    if $DOCKER_COMPOSE -f "$COMPOSE_FILE" exec -T api alembic upgrade head; then
        echo "      迁移完成 ✓"
    else
        echo "      迁移失败，查看最近日志..."
        $DOCKER_COMPOSE -f "$COMPOSE_FILE" logs --tail=30 api
        exit 1
    fi

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  部署完成"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "  API 地址:    http://124.220.70.210:8000"
    echo "  Admin 面板:  http://124.220.70.210:8000/admin"
    echo "  Health:      http://124.220.70.210:8000/health"
    echo ""

    # 显示容器状态
    $DOCKER_COMPOSE -f "$COMPOSE_FILE" ps api worker postgres redis timescaledb 2>/dev/null || true
REMOTE_SCRIPT

success "全部完成！"
