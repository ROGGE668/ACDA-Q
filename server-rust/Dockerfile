FROM rust:1.75-slim-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev libpq-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./

# 创建空 src 目录以缓存依赖编译
RUN mkdir src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src

# 复制真实源码并构建
COPY src ./src
COPY benches ./benches
COPY migrations ./migrations

RUN cargo build --release --bin acda-q-server

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates libssl3 libpq5 curl python3 python3-pip python3-venv \
    && rm -rf /var/lib/apt/lists/*

# Install Python dependencies for sandbox_runner.py
RUN pip3 install --break-system-packages --no-cache-dir pandas sqlalchemy psycopg2-binary

RUN useradd -m -u 1000 quant

COPY --from=builder /app/target/release/acda-q-server /app/acda-q-server
COPY --from=builder /app/migrations /app/migrations
COPY sandbox_runner.py /app/sandbox_runner.py

RUN mkdir -p /app/reports && chown quant:quant /app/reports

USER quant

EXPOSE 8000
ENV RUST_LOG=info
ENV RUST_BACKTRACE=${RUST_BACKTRACE:-0}

CMD ["/app/acda-q-server"]
