CACHE=-v apex-cargo-cache:/usr/local/cargo/registry
RUST_IMG=rust:1.90-slim
RUN_RUST=docker run --rm -v "$(CURDIR)":/app $(CACHE) -w /app $(RUST_IMG)

## ---- Desarrollo (todo en Docker, no necesitas Rust local) ----

check:
	$(RUN_RUST) cargo check --all-targets

test:
	$(RUN_RUST) cargo test

quality:
	$(RUN_RUST) sh -c "rustup component add clippy rustfmt >/dev/null 2>&1; ./quality-check.sh"

build:
	$(RUN_RUST) cargo build --release

# Imagen final liviana (para distribución)
image:
	docker build -t apex-mcp .

## ---- Ejecución ----

# Servidor MCP por stdio usando el binario ya compilado (target/release)
# Uso desde un cliente MCP: command = "make", args = ["-C", "<esta ruta>", "run"]
run:
	@docker run -i --rm \
	  --add-host host.docker.internal:host-gateway \
	  --env-file .env \
	  -v "$(CURDIR)":/app -w /app $(RUST_IMG) /app/target/release/apex-mcp

# Igual pero con la imagen final (tras `make image`)
run-image:
	@docker run -i --rm \
	  --add-host host.docker.internal:host-gateway \
	  --env-file .env \
	  apex-mcp

# Prueba E2E por JSON-RPC (sin LLM): ontología real + guardrails
e2e:
	python3 test/e2e_stdio.py

# Inspector oficial de MCP (requiere node): UI para probar tools a mano
inspector:
	npx @modelcontextprotocol/inspector -- docker run -i --rm \
	  --add-host host.docker.internal:host-gateway \
	  --env-file .env \
	  -v "$(CURDIR)":/app -w /app $(RUST_IMG) /app/target/release/apex-mcp

.PHONY: check test quality build image run run-image e2e inspector
