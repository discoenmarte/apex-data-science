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

.PHONY: help setup graph-up graph-browser graph-down demo demo-db demo-profile demo-clean \
        vpn vpn-forward profile mcp-build mcp-test mcp-quality mcp-inspector
