# ACDA-Q Rust 后端构建
# 多阶段构建，最终镜像极小

FROM rust:1.75-slim as builder

WORKDIR /app

# 安装依赖
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# 复制并构建依赖（利用 Docker 缓存）
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release 2>/dev/null || true

# 复制源码并构建
COPY src ./src
RUN touch src/main.rs
RUN cargo build --release

# 运行阶段 — 最小镜像
FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 从构建阶段复制二进制
COPY --from=builder /app/target/release/acda-q /usr/local/bin/acda-q

EXPOSE 8000

ENV RUST_LOG=info

CMD ["acda-q"]
