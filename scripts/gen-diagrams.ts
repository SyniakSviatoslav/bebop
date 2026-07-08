// Generates the conceptual feature schemas (docs/diagrams/schema-*.svg) from the shared
// FEATURE_SCHEMAS definitions in src/schema.ts. Deterministic + reproducible:
//   npx tsx scripts/gen-diagrams.ts
// (also available as `bebop diagrams`).

import * as fs from 'node:fs';
import * as path from 'node:path';
import { flowSchema, FEATURE_SCHEMAS } from '../src/schema.ts';

const OUT = path.resolve('docs/diagrams');
fs.mkdirSync(OUT, { recursive: true });
let count = 0;
for (const [name, def] of Object.entries(FEATURE_SCHEMAS)) {
  const svg = flowSchema(def.steps, { title: def.title, orientation: 'v' });
  fs.writeFileSync(path.join(OUT, `schema-${name}.svg`), svg);
  count++;
}
console.log(`${count} conceptual schemas generated → docs/diagrams/schema-*.svg`);
