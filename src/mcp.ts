// Bebop MCP server — Model Context Protocol over stdio (JSON-RPC 2.0).
//
// Hand-rolled to keep the dependency surface at zero: MCP is a tiny wire protocol (initialize,
// tools/list, tools/call) and Bebop's capabilities are pure functions. This lets any MCP client
// (Claude Desktop, Cursor, Zed, VS Code, Hermes) drive Bebop as a tool provider without a SDK.
//
// Run it:  bebop mcp            (or: node bebop.ts mcp)
// Wire it into a client:
//   { "mcpServers": { "bebop": { "command": "bebop", "args": ["mcp"] } } }
//
// The server is a thin, deterministic adapter: each tool delegates to a pure module
// (guard / memory / governor / router / knowledge). Fail-closed — a tool that throws returns a
// JSON-RPC error, never an unhandled crash.

import process from 'node:process';
import { selfTest } from './guard.ts';
import { recall, rememberLocal } from './knowledge.ts';
import { Governor } from './governor.ts';
import { route, enforceRouting, type TaskClass } from './router.ts';
import { livingMemory } from './memory.ts';
import { selfMaintain } from './consciousness.ts';

interface JsonRpcRequest {
  jsonrpc: '2.0';
  id?: number | string | null;
  method: string;
  params?: any;
}
interface JsonRpcResponse {
  jsonrpc: '2.0';
  id: number | string | null;
  result?: any;
  error?: { code: number; message: string; data?: any };
}

const SERVER_INFO = { name: 'bebop', version: '0.1.0' };

function tool(name: string, description: string, inputSchema: object) {
  return { name, description, inputSchema: { type: 'object', ...(inputSchema as any) } };
}

const TOOLS = [
  tool('bebop_boot', 'Run the guard-OS self-certification. Returns whether the gates deny on red and pass on green. Refuses to lie.', {
    properties: {},
  }),
  tool('bebop_recall', 'Associative recall from Bebop living memory (Vector Symbolic Architecture). Returns nearest concepts.', {
    properties: {
      query: { type: 'string', description: 'The recall query.' },
      k: { type: 'number', description: 'Number of nearest hits (default 5).' },
    },
    required: ['query'],
  }),
  tool('bebop_remember', 'Write a concept into living memory. Returns the memory id.', {
    properties: {
      concept: { type: 'string', description: 'Concept name.' },
      payload: { type: 'string', description: 'The payload to remember.' },
    },
    required: ['concept', 'payload'],
  }),
  tool('bebop_govern', 'Run the math-proven telemetry governor over a quality stream (0..1). Returns per-step authority, factor health, resonance risk, anomalies.', {
    properties: {
      samples: { type: 'string', description: 'Comma/space-separated quality samples 0..1, e.g. "0.9,0.6,0.2,0.95".' },
    },
    required: ['samples'],
  }),
  tool('bebop_route', 'Classify a task and return the cheapest-adequate backend routing decision.', {
    properties: {
      taskClass: { type: 'string', enum: ['read', 'write', 'reason', 'creativity', 'exec', 'doer', 'redline'], description: 'Task class.' },
    },
    required: ['taskClass'],
  }),
  tool('bebop_self_maintain', 'Run Bebop self-maintenance (test harness + invariant check). Returns health summary.', {
    properties: {},
  }),
];

class InvalidParamsError extends Error {}

function invalid(message: string): never {
  throw new InvalidParamsError(message);
}

function callTool(name: string, params: any): any {
  switch (name) {
    case 'bebop_boot': {
      const t = selfTest();
      return { ok: t.ok, log: t.log, certified: t.ok };
    }
    case 'bebop_recall': {
      const q = params?.query;
      if (typeof q !== 'string' || q.trim().length === 0) invalid('query must be a non-empty string');
      const r = recall(q);
      return { note: r.note, hits: r.hits };
    }
    case 'bebop_remember': {
      const concept = params?.concept;
      const payload = params?.payload;
      if (typeof concept !== 'string' || concept.trim().length === 0) invalid('concept must be a non-empty string');
      if (typeof payload !== 'string' || payload.length === 0) invalid('payload must be a non-empty string');
      const id = rememberLocal(concept, payload);
      return { id, size: livingMemory().size };
    }
    case 'bebop_govern': {
      const cfg = { kp: 1.4, ki: 0.22, kd: 1.5, iMin: -1, iMax: 1, uMin: 0, uMax: 1, targetQuality: 0.9, deadIC: 0.02, icirVolatile: 0.3, plantM: 1, plantB: 0.6, samplePeriod: 0, anomalyK: 3, maxStep: 1 };
      const gov = new Governor(cfg);
      const raw = String(params?.samples ?? '');
      const samples = raw.split(/[\s,]+/).map(Number).filter((n: number) => !Number.isNaN(n));
      if (samples.length === 0) return { error: 'no samples provided' };
      // Validate quality ∈ [0,1] (parity with CLI): clamp + surface a warning so garbage can't
      // masquerade as authoritative telemetry.
      const outOfRange = samples.filter((q: number) => q < 0 || q > 1 || !Number.isFinite(q));
      const qs = samples.map((q: number) => (q < 0 ? 0 : q > 1 ? 1 : q));
      let anomalies = 0;
      const steps = qs.map((q: number, t: number) => {
        const predicted = t > 0 ? qs[t - 1] : q;
        const st = gov.step({ t, predictedQuality: predicted, actualQuality: q, cost: 1e-18, volume: 100 });
        if (st.anomaly) anomalies++;
        return { t, quality: q, authority: st.authority, factor: st.factorStatus, resonanceRisky: st.resonanceRisky, anomaly: st.anomaly };
      });
      return { steps, anomalies, finalAuthority: gov.authority, outOfRangeCount: outOfRange.length };
    }
    case 'bebop_route': {
      const cls = (params?.taskClass ?? 'doer') as TaskClass;
      const d = route(cls);
      const g = enforceRouting(cls, d.model);
      return { taskClass: cls, model: d.model, rationale: d.rationale, enforced: g.ok, note: g.note };
    }
    case 'bebop_self_maintain': {
      const h = selfMaintain();
      return { ok: h.ok, pass: h.pass, fail: h.fail };
    }
    default:
      throw new Error(`unknown tool: ${name}`);
  }
}

export function handle(req: JsonRpcRequest): JsonRpcResponse {
  const id = req.id ?? null;
  try {
    if (req.method === 'initialize') {
      return { jsonrpc: '2.0', id, result: { protocolVersion: '2024-11-05', capabilities: { tools: {} }, serverInfo: SERVER_INFO } };
    }
    if (req.method === 'notifications/initialized') {
      return { jsonrpc: '2.0', id, result: {} };
    }
    if (req.method === 'tools/list') {
      return { jsonrpc: '2.0', id, result: { tools: TOOLS } };
    }
    if (req.method === 'tools/call') {
      const name = req.params?.name;
      const result = callTool(name, req.params?.arguments ?? {});
      return { jsonrpc: '2.0', id, result: { content: [{ type: 'text', text: JSON.stringify(result, null, 2) }] } };
    }
    return { jsonrpc: '2.0', id, error: { code: -32601, message: `method not found: ${req.method}` } };
  } catch (e: any) {
    if (e instanceof InvalidParamsError) {
      return { jsonrpc: '2.0', id, error: { code: -32602, message: e.message } };
    }
    return { jsonrpc: '2.0', id, error: { code: -32000, message: e?.message ?? 'tool error' } };
  }
}

export async function runMcpServer(): Promise<void> {
  let buf = '';
  const write = (r: JsonRpcResponse) => process.stdout.write(JSON.stringify(r) + '\n');
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk: string) => {
    buf += chunk;
    let nl: number;
    while ((nl = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, nl).trim();
      buf = buf.slice(nl + 1);
      if (!line) continue;
      try {
        const req = JSON.parse(line) as JsonRpcRequest;
        // Notifications have no id and we don't reply to them (except initialized ack handled above).
        if (req.id === null || req.id === undefined) {
          if (req.method === 'notifications/initialized') continue;
          // fire-and-forget notification; ignore
          continue;
        }
        write(handle(req));
      } catch {
        write({ jsonrpc: '2.0', id: null, error: { code: -32700, message: 'parse error' } });
      }
    }
  });
  // Keep the event loop alive; the client closes stdin to end the session.
  await new Promise<void>((resolve) => process.stdin.on('end', () => resolve()));
}
