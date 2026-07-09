/**
 * modules.ts — module registry: VERSIONING, INTER-MODULE RELATION GRAPH, and a BOUNDED LOCAL
 * CHANGE-LOG (the operator's "expand modules with versioning, relation graphs, smaller local memory
 * of changes"). This consolidates what was scattered across scripts + ad-hoc bookkeeping into ONE
 * reusable, testable module (FLAG-OFF seam: construct a ModuleRegistry explicitly).
 *
 * What this replaces / consolidates:
 *   • version drift was implicit (package.json only) — now every module carries an explicit version
 *     + a content-addressed snapshot, so a change is detectable WITHOUT re-reading the whole tree.
 *   • relation graph was only built transiently inside arch-mine — now it lives in the registry and
 *     is queryable (who depends on X? what is X's blast radius?) without re-scanning.
 *   • local change memory was the agent's scratch only — now each module keeps a RING-BUFFER of the
 *     last K (id, before-hash, after-hash, tick) diffs: a minimal, bounded, replayable history.
 *
 * Deterministic, pure, no RNG/Date (tick is supplied by the caller — the memory clock, not wallclock).
 * Falsifiable RED+GREEN.
 */

import { buildAdjacency, causalCounterfactual, pointsOfFailure, mineGraph, type MineReport } from './analytics/arch-mine.ts';
import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import path from 'node:path';
import { addressOf } from '../memory.ts';

export interface ChangeEntry {
  tick: number;
  beforeHash: string;
  afterHash: string;
  note: string;
}

export interface ModuleInfo {
  id: string; // namespaced relpath, e.g. "repo:src/kernel"
  rel: string;
  version: string; // semver-ish; bumped on change
  dependsOn: string[]; // direct importers-of-this (downstream, would break if this changes)
  dependedOnBy: string[]; // direct imports (this module's supply)
  changes: ChangeEntry[]; // bounded ring buffer (local memory of changes)
  hash: string; // content address of current source
}

const CHANGE_CAP = 16; // bounded local memory (ring buffer)

/**
 * Registry of modules with versioning + relation graph + bounded change-log. Built from a repo scan;
 * the relation graph is the arch-mine adjacency, surfaced as queryable directional edges.
 */
export class ModuleRegistry {
  private mods = new Map<string, ModuleInfo>();
  private rel2id = new Map<string, string>();
  private A: number[][] = [];
  private nodeIds: string[] = [];
  report: MineReport | null = null;

  /**
   * Build the registry from a repo root. Deterministic scan (skips node_modules/.git/...). Each module
   * is versioned from an initial content hash; relations come from buildAdjacency.
   */
  static fromRepo(root: string, maxDepth = 10): ModuleRegistry {
    const reg = new ModuleRegistry();
    const modules: { id: string; source: string; isMarkdown?: boolean }[] = [];
    const rels: string[] = [];
    const walk = (dir: string, depth: number) => {
      if (depth > maxDepth) return;
      let entries: string[];
      try { entries = readdirSync(dir); } catch { return; }
      for (const e of entries.sort()) {
        if (['node_modules', '.git', 'target', 'dist', 'build', '.bebop', 'spikes'].includes(e)) continue;
        const full = path.join(dir, e);
        let st; try { st = statSync(full); } catch { continue; }
        if (st.isDirectory()) { walk(full, depth + 1); continue; }
        if (!/\.(ts|tsx|md)$/.test(e) || e.endsWith('.test.ts') || e.endsWith('.d.ts')) continue;
        const rel = path.relative(root, full).replace(/\\/g, '/').replace(/\.(ts|tsx|md)$/, '');
        let source = ''; try { source = readFileSync(full, 'utf8'); } catch { continue; }
        modules.push({ id: `repo:${rel}`, source, isMarkdown: e.endsWith('.md') });
        rels.push(rel);
      }
    };
    walk(root, 0);
    reg.load(modules, rels);
    return reg;
  }

  /** Load from pre-built module list (used by tests without FS). */
  load(modules: { id: string; source: string; isMarkdown?: boolean }[], rels: string[]): void {
    const adj = buildAdjacency(modules);
    this.A = adj.A; this.nodeIds = adj.nodes;
    for (let i = 0; i < adj.nodes.length; i++) {
      const id = adj.nodes[i];
      const rel = rels[i] ?? id.split(':').slice(1).join(':');
      this.rel2id.set(rel, id);
      const hash = addressOf(modules[i]?.source ?? '');
      this.mods.set(id, {
        id, rel, version: '0.0.0',
        dependsOn: [], dependedOnBy: [], changes: [], hash,
      });
    }
    // directional edges: nodes[j][i]>0 means j imports i → i.dependedOnBy+=j ; j.dependsOn+=i
    for (let i = 0; i < this.nodeIds.length; i++) {
      for (let j = 0; j < this.nodeIds.length; j++) {
        if (this.A[j][i] > 0) {
          this.mods.get(this.nodeIds[i])!.dependedOnBy.push(this.nodeIds[j]);
          this.mods.get(this.nodeIds[j])!.dependsOn.push(this.nodeIds[i]);
        }
      }
    }
    this.report = mineGraph(modules);
  }

  get(idOrRel: string): ModuleInfo | undefined {
    return this.mods.get(idOrRel) ?? this.mods.get(this.rel2id.get(idOrRel) ?? '');
  }

  all(): ModuleInfo[] { return [...this.mods.values()]; }

  /** Modules whose contract change would break `id` (its downstream — the blast radius). */
  blastRadius(idOrRel: string): string[] {
    const id = this.get(idOrRel)?.id;
    if (!id) return [];
    const cf = causalCounterfactual({ nodes: this.nodeIds, A: this.A }, id);
    return cf?.broken ?? [];
  }

  /** Direct dependents (1-hop downstream). */
  dependents(idOrRel: string): string[] {
    const id = this.get(idOrRel)?.id;
    if (!id) return [];
    const pof = pointsOfFailure({ nodes: this.nodeIds, A: this.A }, id);
    return pof?.downstream ?? [];
  }

  /**
   * Record a local change: bump version, push a bounded change entry (before/after hash + tick).
   * The change-log is the "smaller local memory of changes" — replayable, content-addressed, O(1).
   */
  recordChange(idOrRel: string, newSource: string, tick: number, note = ''): ModuleInfo | undefined {
    const m = this.get(idOrRel);
    if (!m) return undefined;
    const afterHash = addressOf(newSource);
    if (afterHash === m.hash) return m; // no-op change → no version bump (idempotent)
    const [maj, min, pat] = m.version.split('.').map((x) => Number(x) || 0);
    m.version = `${maj}.${min}.${pat + 1}`; // patch bump on content change
    m.changes.push({ tick, beforeHash: m.hash, afterHash, note });
    if (m.changes.length > CHANGE_CAP) m.changes.shift(); // bounded ring buffer
    m.hash = afterHash;
    return m;
  }

  /** Bounded change-log for a module (oldest-first within the cap). */
  changesOf(idOrRel: string): ChangeEntry[] {
    return this.get(idOrRel)?.changes ?? [];
  }
}
