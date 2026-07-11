# ---- Builder ----
FROM rust:1.90-slim AS builder
WORKDIR /app

# Cache de dependencias: compilar deps con un main vacío primero
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
# Forzar recompilación del binario (no de las deps)
RUN touch src/main.rs && cargo build --release

# ---- Runtime ----
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/apex-profiler /usr/local/bin/apex-profiler
# Directorio para el export JSON (montado como volumen)
RUN mkdir -p /app/data
ENV EXPORT_DIR=/app/data
ENTRYPOINT ["apex-profiler"]
