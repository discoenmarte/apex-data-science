# APEX DS Pipeline — root orchestrator.
# Everything runs in Docker; you do NOT need Rust/Neo4j/Postgres installed locally.

PROFILER = apex-profiler
MCP      = apex-mcp
RUST_IMG = rust:1.90-slim

.DEFAULT_GOAL := help

help:
	@echo "APEX DS Pipeline — common commands"
	@echo ""
	@echo "  ONE-TIME"
	@echo "    make setup           Install docker compose plugin (Colima) if missing"
	@echo ""
	@echo "  DEMO (no VPN, ~5 min) — see the whole pipeline work end to end"
	@echo "    make demo            Neo4j + seeded Postgres -> profile -> load graph -> MCP e2e"
	@echo "    make demo-clean      Tear down demo containers and volumes"
	@echo ""
	@echo "  GRAPH"
	@echo "    make graph-up        Start Neo4j (+ GDS plugin)"
	@echo "    make graph-browser   Open Neo4j Browser (localhost:7474)"
	@echo "    make graph-down      Stop Neo4j"
	@echo ""
	@echo "  REAL DATA (needs the client VPN + emailed 2FA code)"
	@echo "    make vpn             Connect the VPN (prompts for password + 2FA)"
	@echo "    make vpn-forward     Port-forward the client Postgres to localhost:5432"
	@echo "    make profile         Profile the real DB -> load the knowledge graph"
	@echo ""
	@echo "  MCP (step 2)"
	@echo "    make mcp-build       Compile the MCP server (release)"
	@echo "    make mcp-test        Unit tests (SQL policy)"
	@echo "    make mcp-quality     fmt + clippy -D warnings + check + test"
	@echo "    make mcp-inspector   Open MCP Inspector UI (needs Node)"
	@echo ""
	@echo "  ASK INSIGHTS FROM YOUR TERMINAL (needs 'claude login' once)"
	@echo "    make ask q=\"...\"     One question -> insight (uses the MCP tools)"
	@echo "    make chat            Interactive session with the MCP tools loaded"
	@echo ""
	@echo "  SHARE THE MCP (URL + API key, for teammates/agents)"
	@echo "    make mcp-key         Generate/show the API key"
	@echo "    make mcp-serve       Start the MCP server over HTTP on :8977"
	@echo "    make mcp-connect     Print ready-to-paste connection commands"
	@echo "    make mcp-stop        Stop the HTTP server"

## ---------- one-time ----------
setup:
	@if docker compose version >/dev/null 2>&1; then \
	  echo "docker compose OK"; \
	else \
	  echo "Installing docker-compose plugin (Colima)..."; \
	  brew install docker-compose; \
	  mkdir -p ~/.docker/cli-plugins; \
	  ln -sfn "$$(brew --prefix)/opt/docker-compose/bin/docker-compose" ~/.docker/cli-plugins/docker-compose; \
	  docker compose version; \
	fi

## ---------- graph ----------
graph-up:
	$(MAKE) -C $(PROFILER) up

graph-browser:
	$(MAKE) -C $(PROFILER) browser

graph-down:
	$(MAKE) -C $(PROFILER) down

## ---------- demo (no VPN) ----------
demo: graph-up demo-db demo-profile mcp-build
	@echo ""
	@echo ">>> Running MCP end-to-end test against the demo graph + demo DB..."
	@DB_NAME=demo DB_USER=postgres DB_PASSWORD=demo DB_PORT=55432 python3 $(MCP)/test/e2e_stdio.py
	@echo ""
	@echo ">>> Demo complete. Explore the graph at http://localhost:7474 (neo4j / apexpassword)."

demo-db:
	@echo ">>> Starting seeded demo Postgres on localhost:55432..."
	@docker rm -f apex-demo-db >/dev/null 2>&1 || true
	@docker run -d --name apex-demo-db -e POSTGRES_PASSWORD=demo -e POSTGRES_DB=demo \
	  -p 55432:5432 postgres:17-alpine >/dev/null
	@until docker exec apex-demo-db pg_isready -U postgres -q 2>/dev/null; do sleep 1; done
	@sleep 2
	@docker exec -i apex-demo-db psql -U postgres -d demo -q < $(PROFILER)/test/seed_demo.sql
	@echo "    demo DB ready."

demo-profile:
	@echo ">>> Profiling the demo DB into Neo4j..."
	@cd $(PROFILER) && docker compose run --rm \
	  -e DB_HOST=host.docker.internal -e DB_PORT=55432 \
	  -e DB_USER=postgres -e DB_PASSWORD=demo -e DB_NAME=demo \
	  profiler

demo-clean:
	@docker rm -f apex-demo-db >/dev/null 2>&1 || true
	$(MAKE) -C $(PROFILER) clean
	@echo "demo torn down."

## ---------- real data (VPN) ----------
vpn:
	$(MAKE) -C $(PROFILER) vpn-connect

vpn-forward:
	$(MAKE) -C $(PROFILER) vpn-forward

profile:
	$(MAKE) -C $(PROFILER) profile

## ---------- mcp ----------
mcp-build:
	$(MAKE) -C $(MCP) build

mcp-test:
	$(MAKE) -C $(MCP) test

mcp-quality:
	$(MAKE) -C $(MCP) quality

mcp-inspector:
	$(MAKE) -C $(MCP) inspector

## ---------- ask insights from the terminal ----------
# Requires the claude CLI ('curl -fsSL https://claude.ai/install.sh | bash' + 'claude login' once)
CLAUDE = PATH="$$HOME/.local/bin:$$PATH" claude
APEX_PROMPT = You have MCP tools from the 'apex' server (a knowledge graph of the client database \
plus guarded read-only SQL). Flow: search_tables -> describe_table/join_path -> aggregate_table \
or run_read_query for real numbers (needs the VPN tunnel up). Ontology tools always work. \
Answer concisely, in the language of the question, citing the tables you used.

ask:
	@test -n "$(q)" || { echo 'usage: make ask q="your question"'; exit 1; }
	@$(CLAUDE) -p "$(q)" \
	  --mcp-config .mcp.json \
	  --allowedTools "mcp__apex" \
	  --append-system-prompt "$(APEX_PROMPT)"

chat:
	@$(CLAUDE) --mcp-config .mcp.json \
	  --allowedTools "mcp__apex" \
	  --append-system-prompt "$(APEX_PROMPT)"

## ---------- share the MCP over HTTP (URL + API key) ----------
MCP_ENV = $(MCP)/.env
MCP_PORT = 8977

mcp-key:
	@grep -q '^MCP_API_KEY=' $(MCP_ENV) 2>/dev/null || \
	  echo "MCP_API_KEY=$$(openssl rand -hex 24)" >> $(MCP_ENV)
	@grep '^MCP_API_KEY=' $(MCP_ENV) | cut -d= -f2

mcp-serve: mcp-key
	@docker rm -f apex-mcp-http >/dev/null 2>&1 || true
	@docker run -d --name apex-mcp-http \
	  --add-host host.docker.internal:host-gateway \
	  --env-file $(MCP_ENV) -e MCP_TRANSPORT=http -e MCP_PORT=$(MCP_PORT) \
	  -p $(MCP_PORT):$(MCP_PORT) \
	  -v "$(CURDIR)/$(MCP)":/app -w /app $(RUST_IMG) /app/target/release/apex-mcp >/dev/null
	@sleep 2 && docker logs apex-mcp-http 2>&1 | tail -1
	@echo "MCP server up:  http://localhost:$(MCP_PORT)/mcp   (key: make mcp-key)"

mcp-connect: mcp-key
	@KEY=$$(grep '^MCP_API_KEY=' $(MCP_ENV) | cut -d= -f2); \
	echo ""; \
	echo "── Claude Code (one command) ────────────────────────────────"; \
	echo "claude mcp add apex --transport http http://localhost:$(MCP_PORT)/mcp --header \"Authorization: Bearer $$KEY\""; \
	echo ""; \
	echo "── Any MCP client (JSON) ────────────────────────────────────"; \
	echo '{ "mcpServers": { "apex": { "type": "http", "url": "http://localhost:$(MCP_PORT)/mcp",'; \
	echo "    \"headers\": { \"Authorization\": \"Bearer $$KEY\" } } } }"; \
	echo ""; \
	echo "── Quick check ──────────────────────────────────────────────"; \
	echo "curl -s -o /dev/null -w '%{http_code}\\n' http://localhost:$(MCP_PORT)/mcp   # 401 = auth on"

mcp-stop:
	@docker rm -f apex-mcp-http >/dev/null 2>&1 || true
	@echo "MCP HTTP server stopped."

.PHONY: help setup graph-up graph-browser graph-down demo demo-db demo-profile demo-clean \
        vpn vpn-forward profile mcp-build mcp-test mcp-quality mcp-inspector \
        ask chat mcp-key mcp-serve mcp-connect mcp-stop
