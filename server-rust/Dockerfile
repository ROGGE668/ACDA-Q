# 多阶段构建：Rust 后端服务

# 阶段 1：Builder
FROM rust:1.75-slim-bookworm AS builder

WORKDIR /app

# 安装依赖
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# 复制依赖文件并缓存
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches
COPY migrations ./migrations

# 构建发布版本
RUN cargo build --release --bin acda-q-server

# 阶段 2：Runtime
FROM debian:bookworm-slim

WORKDIR /app

# 安装运行时依赖
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    && rm -rf /var/lib/apt/lists/*

# 创建非 root 用户
RUN useradd -m -u 1000 quant

# 复制二进制文件
COPY --from=builder /app/target/release/acda-q-server /app/acda-q-server
COPY --from=builder /app/migrations /app/migrations

# 创建报告目录
RUN mkdir -p /app/reports && chown quant:quant /app/reports

USER quant

EXPOSE 8000

ENV RUST_LOG=info
ENV RUST_BACKTRACE=1

CMD ["/app/acda-q-server"]
