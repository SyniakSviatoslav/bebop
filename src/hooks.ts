// Bebop hooks — Claude Code's PreToolUse/PostToolUse/Stop analogue, over the guard OS.
//
// A hook is a shell command that runs at a lifecycle event with JSON on stdin. It may return a
// decision on stdout:
//   { "permissionDecision": "deny", "permissionDecisionReason": "..." }   → block the action
//   { "decision": "block", "reason": "..." }                              → block (bebop-native)
// Exit code 2 also blocks (Claude Code convention). Exit 0 with no decision = allow (pass through).
//
// Events: PreToolUse (before a tool runs — can deny), PostToolUse (after success), Stop (turn end).
//
// Pure & testable: the command runner is injectable so tests assert deny/allow without spawning
// shells. The default runner uses child_process.

import { spawnSync } from 'node:child_process';

export type HookEvent = 'PreToolUse' | 'PostToolUse' | 'Stop';

export interface HookContext {
  event: HookEvent;
  tool?: string; // tool name for PreToolUse/PostToolUse
  args?: Record<string, any>;
  reason?: string;
}

export interface HookDecision {
  blocked: boolean;
  reason?: string;
}

export interface HookSpec {
  matcher?: string; // tool name to match, or "*" / undefined (all)
  command: string;
  // injectable runner for tests; default spawns the command.
  run?: (command: string, input: string) => { code: number; stdout: string };
}

function matches(matcher: string | undefined, tool: string | undefined): boolean {
  if (!matcher || matcher === '*') return true;
  return matcher === tool;
}

function defaultRun(command: string, input: string): { code: number; stdout: string } {
  const r = spawnSync(command, { input, shell: true, encoding: 'utf8', timeout: 10_000 });
  return { code: r.status ?? 0, stdout: r.stdout ?? '' };
}

function parseDecision(stdout: string): HookDecision {
  const trimmed = stdout.trim();
  if (!trimmed) return { blocked: false };
  try {
    const j = JSON.parse(trimmed);
    if (j?.hookSpecificOutput?.permissionDecision === 'deny') {
      return { blocked: true, reason: j.hookSpecificOutput.permissionDecisionReason };
    }
    if (j?.permissionDecision === 'deny') {
      return { blocked: true, reason: j.permissionDecisionReason };
    }
    if (j?.decision === 'block') {
      return { blocked: true, reason: j.reason };
    }
  } catch {
    /* not JSON → no decision */
  }
  return { blocked: false };
}

// Run all hooks for an event; the FIRST deny wins (fail-closed). Returns the combined decision.
export function runHooks(
  specs: HookSpec[],
  ctx: HookContext,
  injectedRun?: (command: string, input: string) => { code: number; stdout: string },
): HookDecision {
  const run = injectedRun ?? defaultRun;
  for (const spec of specs) {
    if (!matches(spec.matcher, ctx.tool)) continue;
    const input = JSON.stringify({ event: ctx.event, tool: ctx.tool, args: ctx.args ?? {}, reason: ctx.reason ?? '' });
    let res: { code: number; stdout: string };
    try {
      res = (spec.run ?? run)(spec.command, input);
    } catch {
      // A hook that crashes is treated as deny (fail-closed) — matches Claude Code's exit-2 semantics.
      return { blocked: true, reason: 'hook error' };
    }
    if (res.code === 2) return { blocked: true, reason: 'blocked by hook (exit 2)' };
    const d = parseDecision(res.stdout);
    if (d.blocked) return { blocked: true, reason: d.reason };
  }
  return { blocked: false };
}

export function preToolUse(specs: HookSpec[], tool: string, args?: Record<string, any>): HookDecision {
  return runHooks(specs, { event: 'PreToolUse', tool, args });
}
