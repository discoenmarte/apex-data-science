# APEX Data Science Pipeline

Turn a huge production database (thousands of tables, terabytes) into a **knowledge graph**
that any AI agent can query for business insights in seconds — **without copying the data**.

We profile the database reading **only schema metadata** (tables, columns, foreign keys),
store that as a graph in Neo4j, and expose it over the **Model Context Protocol (MCP)**.
When an agent needs real numbers, it runs a **single guarded, read-only query** against the
source at runtime. Federation, not replication.

Everything runs in **Docker** — you don't need Rust, Neo4j, or Postgres installed locally.

---

## 1. Run from scratch

### 1.1 Prerequisites (macOS, one time)

```bash
brew install colima docker && colima start   # Docker engine (skip if you use Docker Desktop)
cd ds_pipeline
make setup                                    # installs the docker compose plugin if missing
```

### 1.2 Pick the LLM that drives the questions

The agent brain is **any OpenAI-compatible model**. It's configured in one file — copy the
template and uncomment the block for your setup:

```bash
cp llm.env.example llm.env
```

`llm.env` has three keys:

```ini
LLM_BASE_URL=http://localhost:11434/v1   # where the model server listens
LLM_MODEL=gpt-oss:20b                     # the model id
LLM_API_KEY=ollama                        # ignored by local servers; real key for cloud
```

Ready-made presets are in `llm.env.example`. Common cases:

| Your setup | LLM_BASE_URL | LLM_MODEL | LLM_API_KEY |
|---|---|---|---|
| **Ollama** (local, free — default) | `http://localhost:11434/v1` | `gpt-oss:20b` | `ollama` |
| **LM Studio** (local GUI) | `http://localhost:1234/v1` | *(loaded model)* | `lm-studio` |
| **vLLM / llama.cpp** (self-hosted) | `http://HOST:8000/v1` | *(served id)* | `not-needed` |
| **OpenAI** | `https://api.openai.com/v1` | `gpt-4o` | `sk-...` |

If you use **Ollama** and have nothing yet, this installs it and pulls the default model:

```bash
make llm-setup      # installs Ollama, pulls gpt-oss:20b (~13 GB), creates llm.env
```

> **Your coworker's own model:** he just edits `llm.env` to point at his server
> (`LLM_BASE_URL` + `LLM_MODEL`). No code changes. Any endpoint that speaks the OpenAI
> chat-completions + tool-calls API works. A model with **good tool-calling** is required
> (gpt-oss:20b, qwen2.5:14b+, llama3.1:8b+, gpt-4o, etc.); tiny models won't drive the tools well.

### 1.3 Build the knowledge graph

**Option A — demo on fake data (no VPN, ~5 min):** proves the whole pipeline works.

```bash
make demo           # Neo4j → seeded Postgres → profile → graph → MCP self-test (ALL PASS)
```

**Option B — the real client database (needs the VPN + emailed 2FA code):**

```bash
make graph-up                    # start Neo4j + GDS

# Terminal A (leave running) — prompts for VPN password, then the 2FA code:
make vpn
# Terminal B — once the VPN is connected:
make vpn-forward                 # client Postgres → localhost:5432
make profile                     # profiles ~4,500 tables → fills the graph (2-3 min)
```

### 1.4 Ask questions from the terminal

```bash
make mcp-build                   # compile the MCP server (once)
make chat                        # interactive: type a phrase, get an insight
# or one-shot:
make ask q="¿Qué empresas le deben plata este mes al departamento de oncología?"
```

You'll see the agent's tool calls live (searching the graph, planning joins, running SQL),
then the answer with the tables it used and the exact SQL. Graph questions work offline;
real numbers need the VPN tunnel (1.3 Option B) up.

---

## 2. Connect the MCP (for teammates / agents)

The MCP server speaks two transports. **stdio** for a local client on the same machine,
**HTTP + API key** for anyone else (your coworker's ADK Rust agent graph, another Claude, etc.).

### HTTP + key — the shareable way

```bash
make mcp-serve       # starts the server on http://localhost:8977/mcp (auto-generates the key)
make mcp-connect     # prints the ready-to-paste connection commands with the key filled in
make mcp-stop        # stop it
```

`make mcp-connect` outputs both forms:

```bash
# Claude Code — one command:
claude mcp add apex --transport http http://localhost:8977/mcp \
  --header "Authorization: Bearer <KEY>"

# Any MCP client — JSON config:
{ "mcpServers": { "apex": { "type": "http", "url": "http://localhost:8977/mcp",
    "headers": { "Authorization": "Bearer <KEY>" } } } }
```

- Every request without the key returns `401`.
- The key lives in `apex-mcp/.env` (`MCP_API_KEY`, gitignored). Rotate it: delete that line, run `make mcp-key`.
- Beyond localhost: expose port `8977` on your LAN or tunnel it — same URL + key.

### stdio — local client, no network

Copy `.mcp.json.example` to `.mcp.json`, fix the absolute path, open Claude Code in this
folder — the `apex` tools load automatically. Also works with the official MCP Inspector:
`make mcp-inspector`.

### The 9 tools

| Tool | Backend | What it returns |
|---|---|---|
| `search_tables` | graph | Relevant tables for a business term (fulltext + FK centrality) |
| `describe_table` | graph | Columns, types, stats, PK/FKs |
| `join_path` | graph | Shortest FK path between two tables, with exact column pairs |
| `table_neighborhood` | graph | A table's incoming/outgoing FKs |
| `domain_overview` | graph | Schemas + biggest / most-connected tables |
| `aggregate_table` | source | Templated `count/sum/avg/min/max` + group by |
| `run_read_query` | source | A validated single `SELECT` |
| `sample_rows` | source | Raw rows — **off by default** (clinical data) |
| `source_status` | source | Is the VPN tunnel reachable? latency |

---

## 3. Architecture

### The pipeline

```
                         ┌─ apex-profiler (Rust) ──────────────┐
  Client Postgres ──────▶│ reads pg_catalog ONLY (no row data) │──▶ Neo4j + GDS
  (TBs, 1000s tables)    │ profiles tables in parallel (tokio) │    (the graph:
        ▲                └─────────────────────────────────────┘     tables, columns, FKs)
        │                                                                    │
        │ ③ guarded SELECT                          ② ontology query (bolt)  │
        │    (READ ONLY, 15s, LIMIT, semaphore)     search / describe / join │
        │                                                                    ▼
        └──────────────────────────── apex-mcp (Rust, rmcp) ◀──── ① MCP tool calls ── Agents
                                       stdio + HTTP(+key)                              (ADK Rust,
                                                                                       Claude, ...)
```

Two Rust services, both DDD-layered (`domain` / `application` / `infrastructure`), both
Docker-only:

- **`apex-profiler` (step 1)** — connects to the source, reads `pg_class`, `pg_attribute`,
  `pg_constraint`, `pg_stats` (metadata, **never table rows**), profiles every table
  concurrently with a bounded `tokio` fan-out, and writes a graph to Neo4j:
  `(:Table)-[:HAS_COLUMN]->(:Column)` and `(:Table)-[:REFERENCES]->(:Table)`.
- **`apex-mcp` (step 2)** — an MCP server exposing the 9 tools. Graph tools hit Neo4j
  (local, milliseconds); data tools hit the source through the VPN (seconds).

### The workflow of one question

Example: *"What companies owe money to oncology this month?"*

1. **Agent → `search_tables("oncología")`, `search_tables("cartera")`…** — the graph narrows
   ~4,500 tables to the relevant handful. Fulltext (Lucene) + FK-centrality ranking, accent-folded.
2. **`describe_table` + `join_path`** — the agent learns real columns and the exact foreign-key
   path to connect billing ↔ companies ↔ department. `shortestPath` (≤6 hops) does the work,
   so the LLM never guesses joins.
3. **`aggregate_table` / `run_read_query`** — one query runs against the live DB, returns the
   numbers. This is the only step that touches real data, and only for what was asked.
4. **Agent answers** with the figures, the tables used, and the SQL.

Cheap phase (graph) picks the targets; expensive phase (source) fetches only those numbers.
That's what makes it scale to terabytes: **we never pre-load or duplicate the database.**

### Why the graph in the middle

- **Routing at scale** — finding the right 8 tables out of 4,500 is a graph query, not an LLM scan.
- **Correct joins** — foreign keys are edges; the join plan is a path, computed, not hallucinated.
- **Privacy** — the graph is schema only; patient data stays in the source and is read on demand.
- **Model-agnostic** — the tools are the contract. Any agent (your coworker's ADK Rust graph,
  Claude, a local model) plugs into the same MCP server. Swapping Neo4j for embeddings, or
  adding a ClickHouse cache, changes nothing on the agent side.

### Safety (the source is a clinical DB in production)

Every source query is **AST-validated** (single `SELECT`/`WITH`, no DDL/DML, no multiple
statements), runs in a **`READ ONLY` transaction** with a **15s timeout** and a **row cap**,
and a **semaphore** bounds concurrency so a swarm of agents can't overload the client's DB.
Raw-row sampling is disabled by default; aggregates are the intended path. Every call is audited.

### Layout

| Folder | Role | Stack |
|---|---|---|
| [`apex-profiler/`](apex-profiler/) | Step 1 — parallel schema profiler → Neo4j | Rust, tokio, neo4rs |
| [`apex-mcp/`](apex-mcp/) | Step 2 — MCP server (graph tools + guarded SQL) | Rust, rmcp, sqlparser |
| [`db-vpn-connect/`](db-vpn-connect/) | VPN + port-forward to reach the client DB | Docker, openconnect |
| [`tools/ask.py`](tools/ask.py) | Model-agnostic reference client (~200 lines, stdlib) | Python |

Design deep-dive: [`apex-mcp/DESIGN.md`](apex-mcp/DESIGN.md).
Diagrams: [`apex-mcp/diagrama-simple-apex-mcp.pdf`](apex-mcp/diagrama-simple-apex-mcp.pdf).

### Roadmap

| Step | Status | What |
|---|---|---|
| 1 · Parallel profiling | ✅ | Rust → Neo4j + GDS |
| 2 · MCP over the graph | ✅ | this repo's `apex-mcp` (stdio + HTTP/key) |
| 2.1 · Semantic search | planned | Neo4j vector index + embeddings (same tool signatures) |
| 2.2 · Hot materialization | planned | ClickHouse / R2-Parquet for frequently-asked domains |
| 3 · Freshness | planned | Debezium CDC keeps graph & caches current |

Run `make` (no arguments) for the full command list.
