// Bebop settings — the project/user config file (Claude Code's settings.json analogue).
//
// Loaded from (highest precedence last):
//   1. ~/.bebop/settings.json            (user, all projects)
//   2. <cwd>/bebop.json                 (project, committed-friendly)
//
// Shape (all optional):
//   {
//     "model": "opus" | "haiku" | "sonnet" | "<modelId>",
//     "permissions": {
//       "allow": ["tools/bebop/**", ...],   // globs added to the guard scope
//       "deny":  ["**/secrets/**", ...]      // globs added to the red-line deny set
//     },
//     "hooks": { "PreToolUse": [...], "PostToolUse": [...], "Stop": [...] }
//   }
//
// Pure & testable: loadSettings takes explicit paths so tests never touch the real FS layout.

import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';

export interface HookSpec {
  matcher?: string; // tool name to match (or "*")
  command: string; // shell command; receives JSON on stdin, may print a deny decision
}

export interface BebopSettings {
  model?: string;
  permissions: { allow: string[]; deny: string[] };
  hooks: Record<string, HookSpec[]>;
}

export const EMPTY_SETTINGS: BebopSettings = {
  model: undefined,
  permissions: { allow: [], deny: [] },
  hooks: {},
};

function readJsonSafe(file: string): Partial<BebopSettings> | null {
  try {
    const raw = fs.readFileSync(file, 'utf8');
    return JSON.parse(raw) as Partial<BebopSettings>;
  } catch {
    return null;
  }
}

function mergeSettings(into: BebopSettings, part: Partial<BebopSettings> | null): void {
  if (!part) return;
  if (part.model) into.model = part.model;
  if (part.permissions?.allow) into.permissions.allow.push(...part.permissions.allow);
  if (part.permissions?.deny) into.permissions.deny.push(...part.permissions.deny);
  if (part.hooks) {
    for (const [evt, specs] of Object.entries(part.hooks)) {
      into.hooks[evt] = (into.hooks[evt] ?? []).concat(specs ?? []);
    }
  }
}

export function loadSettings(opts?: {
  cwd?: string;
  userFile?: string;
  projectFile?: string;
}): BebopSettings {
  const cwd = opts?.cwd ?? process.cwd();
  const userFile = opts?.userFile ?? path.join(os.homedir(), '.bebop', 'settings.json');
  const projectFile = opts?.projectFile ?? path.join(cwd, 'bebop.json');
  const s: BebopSettings = JSON.parse(JSON.stringify(EMPTY_SETTINGS));
  mergeSettings(s, readJsonSafe(userFile));
  mergeSettings(s, readJsonSafe(projectFile));
  return s;
}
