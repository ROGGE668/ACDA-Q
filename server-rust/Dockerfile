FROM rust:1.75-slim-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev libpq-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches
COPY migrations ./migrations

RUN cargo build --release --bin acda-q-server

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates libssl3 libpq5 curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 quant

COPY --from=builder /app/target/release/acda-q-server /app/acda-q-server
COPY --from=builder /app/migrations /app/migrations

RUN mkdir -p /app/reports && chown quant:quant /app/reports

USER quant

EXPOSE 8000
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1

CMD ["/app/acda-q-server"]
