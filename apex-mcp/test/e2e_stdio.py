#!/usr/bin/env python3
"""End-to-end test of apex-mcp over JSON-RPC/stdio (same thing MCP Inspector does).

Self-contained demo path: assumes the graph was loaded by profiling the DEMO database
(apex-profiler/test/seed_demo.sql) and that the same DB is reachable for the data tools.
Run it via `make demo` from the repo root, or `make e2e` here after `make demo-setup`.

Env overrides (defaults target the demo DB on port 55432):
  NEO4J_URI, NEO4J_USER, NEO4J_PASSWORD, DB_HOST, DB_PORT, DB_USER, DB_PASSWORD, DB_NAME
"""
import json, os, subprocess, sys, threading, queue

PROJ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def env(k, d):
    return os.environ.get(k, d)

CMD = [
    "docker", "run", "-i", "--rm",
    "--add-host", "host.docker.internal:host-gateway",
    "-v", f"{PROJ}:/app", "-w", "/app",
    "-e", f"NEO4J_URI={env('NEO4J_URI', 'bolt://host.docker.internal:7687')}",
    "-e", f"NEO4J_USER={env('NEO4J_USER', 'neo4j')}",
    "-e", f"NEO4J_PASSWORD={env('NEO4J_PASSWORD', 'apexpassword')}",
    "-e", f"DB_HOST={env('DB_HOST', 'host.docker.internal')}",
    "-e", f"DB_PORT={env('DB_PORT', '55432')}",
    "-e", f"DB_USER={env('DB_USER', 'postgres')}",
    "-e", f"DB_PASSWORD={env('DB_PASSWORD', 'demo')}",
    "-e", f"DB_NAME={env('DB_NAME', 'demo')}",
    "-e", "RUST_LOG=apex_mcp=info",
    "rust:1.90-slim", "/app/target/release/apex-mcp",
]

proc = subprocess.Popen(CMD, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE, text=True, bufsize=1)
q = queue.Queue()
threading.Thread(target=lambda: [q.put(l) for l in proc.stdout], daemon=True).start()
threading.Thread(target=lambda: [sys.stderr.write("  [srv] " + l) for l in proc.stderr], daemon=True).start()

failures = 0

def send(msg):
    proc.stdin.write(json.dumps(msg) + "\n")
    proc.stdin.flush()

def recv(timeout=30):
    return json.loads(q.get(timeout=timeout))

def call(i, name, args):
    send({"jsonrpc": "2.0", "id": i, "method": "tools/call",
          "params": {"name": name, "arguments": args}})
    r = recv()
    is_err = r.get("result", {}).get("isError", False) or "error" in r
    body = r.get("result", {}).get("content", [{}])
    text = body[0].get("text", "") if body else ""
    if "error" in r:
        text = json.dumps(r["error"])
    return is_err, text

def check(cond, label, detail=""):
    global failures
    mark = "PASS" if cond else "FAIL"
    if not cond:
        failures += 1
    print(f"[{mark}] {label}{(': ' + detail) if detail else ''}")

# --- Handshake ---
send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                 "clientInfo": {"name": "apex-e2e", "version": "0.1"}}})
init = recv()
check(init["result"]["serverInfo"]["name"] == "rmcp", "initialize handshake")
send({"jsonrpc": "2.0", "method": "notifications/initialized"})

# --- tools/list ---
send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
tools = recv()["result"]["tools"]
names = sorted(t["name"] for t in tools)
check(len(tools) == 9, "tools/list returns 9 tools", str(names))

# --- Ontology (graph) ---
err, out = call(3, "search_tables", {"term": "cuentas", "top_k": 5})
hits = json.loads(out) if not err else []
check(not err and len(hits) > 0, "search_tables('cuentas')", f"{len(hits)} hits")
for h in hits[:5]:
    print(f"       - {h['fqn']}  score={h['score']:.2f} deg={h['fk_degree']} rows={h['row_estimate']}")

err, out = call(4, "domain_overview", {})
ov = json.loads(out) if not err else {}
check(not err and ov.get("schemas"), "domain_overview",
      f"db={ov.get('database')} schemas={len(ov.get('schemas', []))}")

err, out = call(5, "describe_table", {"fqn": "public.cuentas_detalle"})
det = json.loads(out) if not err else {}
check(not err and det.get("columns"), "describe_table(public.cuentas_detalle)",
      f"{len(det.get('columns', []))} cols, {len(det.get('foreign_keys', []))} FKs")

err, out = call(6, "join_path", {"from_fqn": "public.cuentas_detalle", "to_fqn": "public.cuentas"})
jp = json.loads(out) if not err else {}
check(not err and jp.get("tables"), "join_path(cuentas_detalle -> cuentas)", str(jp.get("tables")))

err, out = call(7, "table_neighborhood", {"fqn": "public.citas"})
nb = json.loads(out) if not err else []
check(not err, "table_neighborhood(public.citas)", f"{len(nb)} FKs")

# --- Data (source, guarded) ---
err, out = call(8, "source_status", {})
st = json.loads(out) if not err else {}
check(not err, "source_status", f"reachable={st.get('reachable')}")

err, out = call(9, "aggregate_table",
                {"table_fqn": "public.citas", "metrics": [{"func": "count"}],
                 "group_by": ["estado"], "limit": 10})
ag = json.loads(out) if not err else {}
check(not err and ag.get("rows"), "aggregate_table(count citas by estado)",
      f"{ag.get('row_count')} groups in {ag.get('elapsed_ms')}ms")

err, out = call(10, "run_read_query",
                {"sql": "SELECT estado, count(*) AS n FROM citas GROUP BY estado ORDER BY n DESC"})
qr = json.loads(out) if not err else {}
check(not err and qr.get("rows"), "run_read_query(citas by estado)",
      f"{qr.get('row_count')} rows in {qr.get('elapsed_ms')}ms")

# --- Guardrails: these MUST be rejected ---
err, _ = call(11, "run_read_query", {"sql": "DELETE FROM citas"})
check(err, "guard rejects DELETE")
err, _ = call(12, "run_read_query", {"sql": "SELECT 1; DROP TABLE citas"})
check(err, "guard rejects multi-statement")
err, _ = call(13, "sample_rows", {"table_fqn": "public.pacientes"})
check(err, "sample_rows disabled by default (clinical data)")

proc.stdin.close()
proc.wait(timeout=10)
print(f"\n{'ALL PASS' if failures == 0 else 'FAILED'} — {failures} failure(s); server exit {proc.returncode}")
sys.exit(1 if failures else 0)
