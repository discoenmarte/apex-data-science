# APEX Data Science Pipeline

Turn a huge, unfamiliar production database into a **knowledge graph** that AI agents can
query for fast business insights — **without ever copying the data**.

The pipeline profiles a client's database (thousands of tables, terabytes) reading **only
schema metadata**, builds a graph of tables/columns/foreign-keys in Neo4j, and exposes it
to any LLM agent over the **Model Context Protocol (MCP)**. When an agent needs real
numbers, it runs a single guarded, read-only query against the source at runtime.
**Federation, not replication.**

Everything runs in **Docker** — you do not need Rust, Neo4j, or Postgres installed on your Mac.

```
Data source (Postgres, TBs)                         Agents (ADK Rust, Claude, ...)
        │                                                       │  MCP tool calls
        │ ① profile (schema only, parallel, Rust)               ▼
        ▼                                              ┌──────────────────┐
  Neo4j + GDS  ◀──────────── ontology (ms, local) ────│    apex-mcp      │
  (the graph)                                          │  (Rust MCP srv)  │
        ▲                                              └────────┬─────────┘
        └───────── ② guarded SELECT (seconds, via VPN) ─────────┘
```

---

## What's in here

| Folder | What it is | Language |
|---|---|---|
| [`apex-profiler/`](apex-profiler/) | **Step 1** — parallel schema profiler. Reads Postgres catalogs, builds the Neo4j graph. | Rust |
| [`apex-mcp/`](apex-mcp/) | **Step 2** — MCP server. Ontology tools over the graph + guarded runtime SQL. | Rust |
| [`db-vpn-connect/`](db-vpn-connect/) | VPN + port-forward helper to reach the client DB (vendored, see [ORIGIN](db-vpn-connect/ORIGIN.md)). | Docker |

Design docs & diagrams live in `apex-mcp/DESIGN.md` and the `*.pdf` files.

---

## Prerequisites (macOS)

1. **Docker** running — [Docker Desktop](https://www.docker.com/products/docker-desktop/) or
   [Colima](https://github.com/abiosoft/colima) (`brew install colima docker && colima start`).
2. That's it. First run pulls images and compiles Rust inside Docker (a few minutes, cached after).

If you use Colima, install the compose plugin once:

```bash
make setup
```

---

## Quick start — see it work in ~5 minutes (no VPN needed)

This spins up Neo4j, seeds a small demo Postgres, profiles it into the graph, and runs the
MCP server end-to-end against it:

```bash
git clone <your-repo-url> && cd ds_pipeline
make setup     # once, if on Colima
make demo      # Neo4j + demo DB -> profile -> load graph -> MCP end-to-end test
```

You should see the MCP tools returning real results (`search_tables`, `join_path`,
`aggregate_table`, ...) and the guardrails rejecting `DELETE`. Then open the graph:

```bash
make graph-browser        # http://localhost:7474  (login: neo4j / apexpassword)
```

Try this in the Neo4j Browser:

```cypher
MATCH (t:Table {name:'cuentas_detalle'})-[r:REFERENCES]-(o:Table) RETURN t, r, o;
```

Tear the demo down when done:

```bash
make demo-clean
```

---

## Full pipeline against the real client database

The real database sits behind a FortiGate SSL-VPN with **2FA** — connecting needs a code
that is emailed at connect time, so a human must be in the loop for that one step.

### 1. Configure secrets

```bash
cp apex-profiler/.env.example              apex-profiler/.env
cp apex-mcp/.env.example                   apex-mcp/.env
cp db-vpn-connect/.env/.client.example  db-vpn-connect/.env/.client
# then edit those three files with the real VPN + Postgres credentials
```

### 2. Start the graph database

```bash
make graph-up
```

### 3. Connect the VPN  (terminal A — leave it running)

```bash
make vpn
# enter the VPN password, then the 2FA code from the email
```

> First connect only: if `openconnect` prints a certificate pin error, copy the
> `pin-sha256:...` value it shows into `SERVER_CERT` in `db-vpn-connect/.env/.client`.

### 4. Port-forward the client Postgres  (terminal B)

```bash
make vpn-forward     # exposes the client DB on localhost:5432
```

### 5. Profile the real database → load the graph

```bash
make profile         # reads schema only; ~2–3 min for thousands of tables
make graph-browser   # explore the result
```

### 6. Build & run the MCP server

```bash
make mcp-build       # compile once
make mcp-inspector   # opens MCP Inspector UI to try the tools by hand (needs Node)
```

---

## Connect the MCP to Claude (or any MCP client)

1. `make mcp-build` (produces `apex-mcp/target/release/apex-mcp`).
2. Copy the template and fix the two absolute paths:

   ```bash
   cp .mcp.json.example .mcp.json
   # edit .mcp.json: replace /ABSOLUTE/PATH/TO/ds_pipeline with this repo's real path
   ```

3. Open Claude Code in this folder — it auto-loads `.mcp.json` and the `apex` tools appear.

Now you can ask, e.g. *"What tables relate to billing, and how would I join them?"* — the
agent uses `search_tables` + `join_path` (instant, from the graph), and `aggregate_table`
for real numbers (only if the VPN is up).

> The ontology tools work **without** the VPN (the graph is local). The data tools
> (`aggregate_table`, `run_read_query`) need the VPN tunnel from steps 3–4.

---

## Ask insights from your terminal

Type a phrase, get an insight — Claude acts as the agent and drives the MCP tools for you.

**One-time setup** (installs the `claude` CLI and logs into your existing Claude account):

```bash
curl -fsSL https://claude.ai/install.sh | bash
claude login
```

**Then, from the repo root:**

```bash
# One-shot question
make ask q="¿Qué tablas tienen que ver con cartera y cómo las joineo?"
make ask q="Who are the top 20 debtors by outstanding balance?"   # needs VPN up

# Or an interactive session (keeps context between questions)
make chat
```

`make ask` runs Claude headless with the `apex` tools pre-approved: it searches the graph,
plans the joins, and (if the VPN tunnel is up) runs the guarded aggregate — then prints
the answer with the tables it used.

---

## Share the MCP connection — URL + API key

To let a teammate or another agent (e.g. the ADK Rust agent graph) connect **without**
knowing anything about Docker or this repo's paths:

```bash
make mcp-serve      # starts the server over HTTP on :8977 (auto-generates the key)
make mcp-connect    # prints ready-to-paste connection commands
```

`make mcp-connect` outputs, with the real key filled in:

```bash
# Claude Code — one command:
claude mcp add apex --transport http http://localhost:8977/mcp \
  --header "Authorization: Bearer <KEY>"

# Any MCP client — JSON config:
{ "mcpServers": { "apex": { "type": "http", "url": "http://localhost:8977/mcp",
    "headers": { "Authorization": "Bearer <KEY>" } } } }
```

Every request without the key gets `401`. The key lives in `apex-mcp/.env`
(`MCP_API_KEY`, gitignored); rotate it by deleting that line and running `make mcp-key`.
To share beyond localhost, expose the port on your LAN or tunnel it — same URL + key.

---

## Security & privacy

- **Schema only** in step 1 — the profiler reads Postgres catalogs (`pg_class`,
  `pg_stats`, ...), never table rows.
- **Read-only at runtime** — every source query is AST-validated (single `SELECT`/`WITH`),
  runs in a `READ ONLY` transaction with a 15s timeout and a row cap, and concurrency is
  capped by a semaphore so agents can't overload the client's production DB.
- **Row sampling is off by default** (`ALLOW_ROW_SAMPLING=false`) because the source is a
  clinical database. Aggregates are the intended path.
- **No secrets in git** — all `.env` / `.client` files are gitignored; commit only the
  `.example` templates.
- **HTTP mode requires a Bearer API key** (`MCP_API_KEY`, ≥16 chars, constant-time
  comparison); every unauthenticated request gets `401`. stdio mode is local-only.

---

## Roadmap

| Step | Status | What |
|---|---|---|
| 1 · Parallel profiling | ✅ done | Rust → Neo4j + GDS |
| 2 · MCP over the graph | ✅ done | this repo's `apex-mcp` |
| 2.1 · Semantic search | planned | Neo4j vector index + embeddings (same tool signatures) |
| 2.2 · Hot materialization | planned | ClickHouse / R2-Parquet backend for frequently-asked domains |
| 3 · Freshness | planned | Debezium CDC keeps the graph & caches current |

## Command reference

Run `make` (no arguments) at the repo root for the full list.
