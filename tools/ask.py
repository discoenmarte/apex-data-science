#!/usr/bin/env python3
"""
ask.py — Model-agnostic natural-language insights over the apex MCP server.

  LLM side : any OpenAI-compatible endpoint (Ollama local by default — free).
  MCP side : apex-mcp over streamable HTTP + Bearer API key (URL + key, nothing else).

Usage:
  python3 tools/ask.py "¿Qué empresas le deben plata este mes al departamento de oncología?"
  python3 tools/ask.py                    # interactive REPL

Env (all optional):
  LLM_BASE_URL  default http://localhost:11434/v1     (Ollama)
  LLM_MODEL     default qwen2.5:14b
  LLM_API_KEY   default 'ollama' (ignored by Ollama; set real key for cloud providers)
  MCP_URL       default http://localhost:8977/mcp
  MCP_KEY       default: read MCP_API_KEY from apex-mcp/.env
"""
import json
import os
import re
import sys
import urllib.error
import urllib.request

BOLD, DIM, CYAN, AMBER, GREEN, RED, END = (
    "\033[1m", "\033[2m", "\033[36m", "\033[33m", "\033[32m", "\033[31m", "\033[0m",
)

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LLM_BASE_URL = os.environ.get("LLM_BASE_URL", "http://localhost:11434/v1").rstrip("/")
LLM_MODEL = os.environ.get("LLM_MODEL", "gpt-oss:20b")
LLM_API_KEY = os.environ.get("LLM_API_KEY", "ollama")
MCP_URL = os.environ.get("MCP_URL", "http://localhost:8977/mcp")
MAX_TURNS = 25

SYSTEM_PROMPT = """You are a senior data analyst working over a knowledge graph of a large \
hospital database (4,500+ tables, Spanish names) plus guarded read-only SQL access to the \
real source. You are persistent: a real analyst never gives up after two searches.

ALWAYS follow this flow — never guess table or column names:
1. search_tables with SHORT Spanish terms, one concept at a time. Fire SEVERAL searches: \
for money/debt questions use 'cartera', 'cuentas', 'facturas', 'recibos', 'saldos', \
'convenios', 'contratos', 'empresas', 'terceros', 'eps', 'aseguradoras'. For clinical \
areas ('oncologia', 'urgencias'...) ALSO consider that a department is often a COLUMN \
(centro de costo, servicio, departamento) in billing tables, not a table itself — check \
matched_columns in results and search 'servicios', 'centros costo', 'departamentos'.
2. If a search returns [], try synonyms immediately — do not conclude anything from one \
empty result.
3. describe_table on the best 3-5 candidates (look at columns AND foreign_keys).
4. join_path between the tables you need to connect — you MUST call it before writing any \
multi-table SQL, and before ever answering "no connection exists".
5. For real numbers: run_read_query with ONE SELECT following the join_path hops \
(date filters like `>= date_trunc('month', CURRENT_DATE)`, GROUP BY, ORDER BY, LIMIT 20). \
Prefer aggregate_table for single-table aggregations.
6. If the source is unreachable (VPN down), say so and still deliver the full analysis: \
relevant tables, the join plan, and the exact SQL ready to run.

NEVER answer "I could not find it" unless you tried at least 6 different search terms and \
inspected the FK neighborhood of the closest tables (table_neighborhood).

Rules: answer in the user's language; be concise; cite tables used (schema.table); show \
the final SQL; never invent numbers — only report what a tool returned."""


# ─────────────────────────── HTTP helpers (stdlib only) ───────────────────────────

def _post(url: str, headers: dict, body: dict, timeout: int = 300):
    req = urllib.request.Request(
        url, data=json.dumps(body).encode(), headers=headers, method="POST"
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.headers, resp.read().decode()


def _sse_json(raw: str, want_id):
    """Extract the JSON-RPC response with the given id from an SSE or JSON body."""
    if raw.lstrip().startswith("{"):
        return json.loads(raw)
    for line in raw.splitlines():
        if line.startswith("data:"):
            payload = line[5:].strip()
            if not payload:
                continue
            try:
                msg = json.loads(payload)
            except json.JSONDecodeError:
                continue
            if msg.get("id") == want_id:
                return msg
    raise RuntimeError(f"no JSON-RPC response id={want_id} in body: {raw[:200]}")


# ─────────────────────────── MCP client (streamable HTTP) ───────────────────────────

class McpHttp:
    def __init__(self, url: str, key: str):
        self.url, self.key, self.session, self._id = url, key, None, 0

    def _headers(self):
        h = {
            "Authorization": f"Bearer {self.key}",
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        }
        if self.session:
            h["mcp-session-id"] = self.session
        return h

    def _rpc(self, method: str, params=None):
        self._id += 1
        body = {"jsonrpc": "2.0", "id": self._id, "method": method}
        if params is not None:
            body["params"] = params
        headers, raw = _post(self.url, self._headers(), body)
        if headers.get("mcp-session-id"):
            self.session = headers["mcp-session-id"]
        msg = _sse_json(raw, self._id)
        if "error" in msg:
            raise RuntimeError(msg["error"].get("message", str(msg["error"])))
        return msg["result"]

    def _notify(self, method: str):
        _post(self.url, self._headers(), {"jsonrpc": "2.0", "method": method})

    def connect(self):
        info = self._rpc("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "apex-ask", "version": "1.0"},
        })
        self._notify("notifications/initialized")
        return info

    def tools(self):
        return self._rpc("tools/list")["tools"]

    def call(self, name: str, args: dict) -> tuple[bool, str]:
        r = self._rpc("tools/call", {"name": name, "arguments": args})
        text = "\n".join(c.get("text", "") for c in r.get("content", []))
        return bool(r.get("isError")), text


# ─────────────────────────── LLM client (OpenAI-compatible) ───────────────────────────

def llm_chat(messages, tools):
    body = {
        "model": LLM_MODEL,
        "messages": messages,
        "tools": tools,
        "temperature": 0.2,
    }
    headers = {"Content-Type": "application/json", "Authorization": f"Bearer {LLM_API_KEY}"}
    try:
        _, raw = _post(f"{LLM_BASE_URL}/chat/completions", headers, body)
    except urllib.error.URLError as e:
        sys.exit(f"{RED}LLM no disponible en {LLM_BASE_URL} — ¿corriste 'make llm-setup'? ({e}){END}")
    return json.loads(raw)["choices"][0]["message"]


def mcp_tools_to_openai(mcp_tools):
    return [{
        "type": "function",
        "function": {
            "name": t["name"],
            "description": t.get("description", ""),
            "parameters": t.get("inputSchema", {"type": "object"}),
        },
    } for t in mcp_tools]


# ─────────────────────────── Agent loop ───────────────────────────

def read_mcp_key() -> str:
    if os.environ.get("MCP_KEY"):
        return os.environ["MCP_KEY"]
    env_path = os.path.join(REPO, "apex-mcp", ".env")
    try:
        with open(env_path) as f:
            for line in f:
                m = re.match(r"^MCP_API_KEY=(.+)$", line.strip())
                if m:
                    return m.group(1)
    except FileNotFoundError:
        pass
    sys.exit(f"{RED}No hay MCP_KEY ni MCP_API_KEY en apex-mcp/.env — corre 'make mcp-serve' primero.{END}")


def summarize(text: str, limit: int = 160) -> str:
    flat = " ".join(text.split())
    return flat[:limit] + ("…" if len(flat) > limit else "")


def answer(question: str, mcp: McpHttp, openai_tools) -> str:
    messages = [
        {"role": "system", "content": SYSTEM_PROMPT},
        {"role": "user", "content": question},
    ]
    for _ in range(MAX_TURNS):
        msg = llm_chat(messages, openai_tools)
        calls = msg.get("tool_calls") or []
        if not calls:
            return msg.get("content") or "(sin respuesta)"
        messages.append(msg)
        for call in calls:
            fn = call["function"]["name"]
            try:
                args = json.loads(call["function"].get("arguments") or "{}")
            except json.JSONDecodeError:
                args = {}
            print(f"{CYAN}  ⚙ {fn}{END}{DIM} {json.dumps(args, ensure_ascii=False)[:120]}{END}",
                  file=sys.stderr)
            try:
                is_err, out = mcp.call(fn, args)
            except Exception as e:  # noqa: BLE001 — el agente debe ver el error y adaptarse
                is_err, out = True, str(e)
            mark = f"{RED}✗{END}" if is_err else f"{GREEN}✓{END}"
            print(f"    {mark} {DIM}{summarize(out)}{END}", file=sys.stderr)
            messages.append({
                "role": "tool",
                "tool_call_id": call.get("id", fn),
                "content": ("ERROR: " if is_err else "") + out[:8000],
            })
    return "(agotado el máximo de pasos — intenta una pregunta más específica)"


def main():
    key = read_mcp_key()
    mcp = McpHttp(MCP_URL, key)
    try:
        mcp.connect()
    except urllib.error.URLError:
        sys.exit(f"{RED}apex-mcp no responde en {MCP_URL} — corre 'make mcp-serve' primero.{END}")
    openai_tools = mcp_tools_to_openai(mcp.tools())
    print(f"{DIM}modelo: {LLM_MODEL} @ {LLM_BASE_URL} · mcp: {MCP_URL} · "
          f"{len(openai_tools)} tools{END}", file=sys.stderr)

    if len(sys.argv) > 1:
        question = " ".join(sys.argv[1:])
        print(f"\n{BOLD}{answer(question, mcp, openai_tools)}{END}")
        return

    print(f"{AMBER}Escribe tu pregunta (o 'salir'):{END}", file=sys.stderr)
    while True:
        try:
            question = input(f"{BOLD}❯ {END}").strip()
        except (EOFError, KeyboardInterrupt):
            break
        if not question or question.lower() in {"salir", "exit", "quit"}:
            break
        print(f"\n{BOLD}{answer(question, mcp, openai_tools)}{END}\n")


if __name__ == "__main__":
    main()
