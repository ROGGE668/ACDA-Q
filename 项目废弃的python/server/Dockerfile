# 构建阶段
FROM python:3.11-slim as builder

WORKDIR /app

# 使用国内 apt 镜像源
RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
RUN pip install --no-cache-dir --user -r requirements.txt -i https://pypi.tuna.tsinghua.edu.cn/simple

# 运行阶段
FROM python:3.11-slim

WORKDIR /app

# 使用国内 apt 镜像源
RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update && apt-get install -y --no-install-recommends \
    libpq5 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# 创建非 root 用户
RUN groupadd -r quant && useradd -r -g quant quant

# 从构建阶段复制依赖
COPY --from=builder /root/.local /home/quant/.local
ENV PATH=/home/quant/.local/bin:$PATH

# 复制应用代码（保持 server 包结构）
RUN mkdir -p /app/server
COPY --chown=quant:quant . /app/server/

ENV PYTHONPATH=/app
ENV PYTHONDONTWRITEBYTECODE=1
ENV PYTHONUNBUFFERED=1

USER quant

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/health || exit 1

CMD ["uvicorn", "server.api.main:app", "--host", "0.0.0.0", "--port", "8000"]
