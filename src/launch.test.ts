import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makePaint } from './theme.ts';
import { playLaunch } from './launch.ts';

// In tests, process.stdout.isTTY is undefined → playLaunch takes the static (non-TTY) branch.
// It must resolve and emit the calm "powered up" line without touching cursor state.
test('GREEN: playLaunch static branch (no TTY) resolves and prints a calm line', async () => {
  const paint = makePaint();
  // capture stdout
  const chunks: string[] = [];
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout.write as any) = (s: string) => { chunks.push(String(s)); return true; };
  try {
    await playLaunch({ paints: paint });
  } finally {
    process.stdout.write = orig;
  }
  const out = chunks.join('');
  assert.ok(out.includes('Bebop ship powered up'), 'static frame should print the powered-up line');
});

test('GREEN: playLaunch honors NO_ANIM skip', async () => {
  const paint = makePaint();
  const prev = process.env.NO_ANIM;
  process.env.NO_ANIM = '1';
  const chunks: string[] = [];
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout.write as any) = (s: string) => { chunks.push(String(s)); return true; };
  try {
    await playLaunch({ paints: paint });
  } finally {
    process.stdout.write = orig;
    if (prev === undefined) delete process.env.NO_ANIM; else process.env.NO_ANIM = prev;
  }
  assert.ok(chunks.join('').includes('powered up'));
});
