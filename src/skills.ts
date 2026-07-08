// Bebop skills — the agent-skills / Claude Code SKILL.md analogue.
//
// A skill is a folder containing SKILL.md with YAML frontmatter + a markdown body:
//   ---
//   name: review
//   description: Run a focused code review pass on the diff.
//   ---
//   # Review
//   ...
//
// Loaded from <cwd>/.bebop/skills/*/SKILL.md (and ~/.bebop/skills/*/SKILL.md). findSkill(query)
// returns the best match by substring over name+description. Pure & testable (injectable dirs).

import fs from 'node:fs';
import path from 'node:path';

export interface Skill {
  name: string;
  description: string;
  body: string;
  dir: string;
}

function parseFrontmatter(raw: string): { meta: Record<string, string>; body: string } {
  const m = raw.match(/^---\s*\n([\s\S]*?)\n---\s*\n?([\s\S]*)$/);
  if (!m) return { meta: {}, body: raw };
  const meta: Record<string, string> = {};
  for (const line of m[1].split('\n')) {
    const idx = line.indexOf(':');
    if (idx < 0) continue;
    const k = line.slice(0, idx).trim();
    const v = line.slice(idx + 1).trim();
    meta[k] = v.replace(/^["']|["']$/g, '');
  }
  return { meta, body: m[2] };
}

function loadDir(dir: string): Skill[] {
  const out: Skill[] = [];
  let entries: fs.Dirent[] = [];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    if (!e.isDirectory()) continue;
    const sk = path.join(dir, e.name, 'SKILL.md');
    try {
      const raw = fs.readFileSync(sk, 'utf8');
      const { meta, body } = parseFrontmatter(raw);
      if (!meta.name) continue;
      out.push({ name: meta.name, description: meta.description ?? '', body, dir: path.join(dir, e.name) });
    } catch {
      /* skip unreadable */
    }
  }
  return out;
}

export function loadSkills(opts?: { cwd?: string; userSkills?: string }): Skill[] {
  const cwd = opts?.cwd ?? process.cwd();
  const userSkills = opts?.userSkills ?? path.join(process.env.HOME ?? process.env.USERPROFILE ?? '', '.bebop', 'skills');
  const projectSkills = path.join(cwd, '.bebop', 'skills');
  // user skills first, project overrides by name
  const byName = new Map<string, Skill>();
  for (const s of loadDir(userSkills)) byName.set(s.name, s);
  for (const s of loadDir(projectSkills)) byName.set(s.name, s);
  return [...byName.values()];
}

export function findSkill(skills: Skill[], query: string): Skill | undefined {
  const q = query.toLowerCase();
  // exact name match wins
  const exact = skills.find((s) => s.name.toLowerCase() === q);
  if (exact) return exact;
  // else best substring over name+description
  let best: Skill | undefined;
  let bestScore = 0;
  for (const s of skills) {
    const hay = `${s.name} ${s.description}`.toLowerCase();
    if (hay.includes(q)) {
      const score = (s.name.toLowerCase().includes(q) ? 2 : 0) + (s.description.toLowerCase().includes(q) ? 1 : 0);
      if (score > bestScore) {
        bestScore = score;
        best = s;
      }
    }
  }
  return best;
}
