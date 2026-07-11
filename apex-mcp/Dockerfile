# ---- Builder ----
FROM rust:1.90-slim AS builder
WORKDIR /app

# Cache de dependencias: compilar deps con un main vacío primero
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

# ---- Runtime ----
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/apex-mcp /usr/local/bin/apex-mcp
# stdio: el cliente MCP habla por stdin/stdout; logs van a stderr
ENTRYPOINT ["apex-mcp"]
