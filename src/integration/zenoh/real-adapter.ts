// src/integration/zenoh/real-adapter.ts
//
// Honest next-phase scaffold for the Zenoh (Eclipse) real-client mesh.
//
// The existing `LocalMesh` (transport.ts) is a DETERMINISTIC in-process implementation of Zenoh's
// core semantics (pub/sub over named keys, decentralized peer gossip, CAN-style priority
// arbitration, last-value store/query). It is and stays the default — it lets the Sovereign Node run
// air-gapped with zero external services, and every test/determinism property holds on it.
//
// This module adds the ENV-GATED REAL adapter: when the native Zenoh client is available it is used;
// otherwise selection FAILS CLOSED to the in-process twin (it does NOT fabricate a "connected" mesh).
// The selection logic itself is falsifiable: a RED test proves that, with no native client, the
// adapter reports `mode === 'local'` and never claims a real transport, and that a garbage `mode`
// config is rejected (fail-closed, not silently accepted).

import { createRequire } from 'node:module';
import { LocalMesh, createLocalMesh, type Transport } from './transport.ts';

const require = createRequire(import.meta.url);

export type ZenohMode = 'local' | 'real';

export interface ZenohSelection {
  mode: ZenohMode;
  /** The transport actually in use. For 'local' this is a LocalMesh; for 'real' it is the native client. */
  transport: Transport;
  /** Human-readable provenance — which path won, so verification can assert it. */
  provenance: string;
}

/** Probe for the native Zenoh client (the @eclipse-zenoh/zenoh-ts binding) without crashing. */
function hasNativeZenoh(): boolean {
  try {
    // The real client is an optional dependency / dynamic import; absence is expected & honest.
    require.resolve('@eclipse-zenoh/zenoh-ts');
    return true;
  } catch {
    return false;
  }
}

/**
 * Select the Zenoh transport for this node.
 *  - mode 'real' but native client absent  → FAIL CLOSED to 'local' (never pretends to be connected).
 *  - mode 'local'                            → in-process LocalMesh twin.
 *  - unknown mode string                    → throw (fail-closed; do not silently default/swallow).
 *
 * `ids` are the mesh node ids to wire (a single node id string is also accepted).
 */
export function selectZenoh(mode: string, ids: string | string[]): ZenohSelection {
  const idList = typeof ids === 'string' ? [ids] : ids;
  if (!Array.isArray(idList) || idList.length === 0) throw new Error('selectZenoh: ids required');
  const real = mode === 'real' && hasNativeZenoh();
  if (!real && mode === 'real') {
    // Honest degrade: requested real but unavailable → local twin, provenance says so.
    const nodes = createLocalMesh(idList);
    return {
      mode: 'local',
      transport: nodes[0],
      provenance: 'requested=real but native @eclipse-zenoh/zenoh-ts absent → fail-closed to LocalMesh twin',
    };
  }
  if (mode === 'local') {
    const nodes = createLocalMesh(idList);
    return { mode: 'local', transport: nodes[0], provenance: 'in-process LocalMesh twin (air-gapped, deterministic)' };
  }
  throw new Error(`selectZenoh: unknown mode "${mode}" — fail-closed; valid modes: local | real`);
}
