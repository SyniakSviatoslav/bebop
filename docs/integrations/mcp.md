# MCP server

Bebop ships a **Model Context Protocol** server over stdio — `bebop mcp`. It exposes Bebop's
capabilities as MCP *tools* to any MCP client (Claude Desktop, Cursor, Zed, VS Code, Hermes).
It is **hand-rolled JSON-RPC 2.0**, so it adds **zero new dependencies**.

## Run it

```bash
bebop mcp
# ◈ Bebop MCP server starting on stdio (JSON-RPC 2.0). Close stdin to stop.
```

Or directly:

```bash
node bebop.ts mcp
```

## Wire it into a client

Add Bebop to your MCP client config:

```json
{
  "mcpServers": {
    "bebop": {
      "command": "bebop",
      "args": ["mcp"]
    }
  }
}
```

For Claude Desktop: `~/Library/Application Support/Claude/claude_desktop_config.json`
(macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows). For Cursor/Zed/VS Code,
use their respective `mcp.json`.

## Tools exposed

| Tool | What it does |
| --- | --- |
| `bebop_boot` | Run the guard-OS self-certification; returns `certified`. |
| `bebop_recall` | Associative recall from living memory (`query`, `k`). |
| `bebop_remember` | Write a concept into living memory (`concept`, `payload`). |
| `bebop_govern` | Run the telemetry governor over a quality stream (`samples`). |
| `bebop_route` | Classify a task and return the cheapest-adequate backend. |
| `bebop_self_maintain` | Run self-maintenance; returns health summary. |

## Protocol

The server speaks JSON-RPC 2.0 over newline-delimited JSON on stdin/stdout:

```
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
← {"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"bebop","version":"0.1.0"}}}

→ {"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
← {"jsonrpc":"2.0","id":2,"result":{"tools":[ ... ]}}

→ {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"bebop_boot","arguments":{}}}
← {"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"{...}"}]}}
```

## Why hand-rolled

MCP is a thin wire protocol. Pulling a full SDK would add a dependency to a project whose whole
point is a portable, dependency-light core. The hand-rolled server is ~200 lines, fully tested
(`mcp.test.ts`), and fail-closed: a tool that throws returns a JSON-RPC error, never an
unhandled crash.

## Extending

Add a `tool(...)` descriptor to `TOOLS` and a `case` in `callTool()` in `src/mcp.ts`. Delegate to
a pure module. Add a RED+GREEN test in `mcp.test.ts`. Done.

## ▶ Live CLI

> Real `bebop` output, recorded with [asciinema](https://asciinema.org) → [agg](https://github.com/asciinema/agg) (no staging, no post-editing).

**bebop mcp — Model Context Protocol server over stdio**

![bebop mcp — Model Context Protocol server over stdio](../footage/feat-mcp.gif)

