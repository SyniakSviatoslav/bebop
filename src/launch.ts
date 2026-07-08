// Bebop launch animation — a red cosmo-noir spaceship lifting off in the terminal.
//
// Best-practice CLI animation (research-backed):
//   - ZERO dependencies. Pure ANSI escape codes (matches theme.ts ESC helpers).
//   - TTY-aware: only animates when process.stdout.isTTY is true. When piped/redirected/CI,
//     it prints a single static frame and returns — no cursor tricks on a non-terminal.
//   - Skippable: honors NO_ANIM / CI env (operators scripting bebop get clean output).
//   - Safe: hides the cursor during play, ALWAYS restores it (try/finally); never leaves the
//     terminal in a broken state. Uses \r + line-clear so it composes with normal logging.
//   - Deterministic timing so it never hangs; self-terminates after N frames.

import type { Paint } from './theme.ts';

export type Paints = { [k: string]: Paint };

const ESC = '\x1b[';
const HIDE_CURSOR = `${ESC}?25l`;
const SHOW_CURSOR = `${ESC}?25h`;
const CLEAR_LINE = `${ESC}2K`;
const UP = (n: number) => `${ESC}${n}A`;

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

// A compact red ship. `flame` toggles between two exhaust lengths for a flicker.
function shipFrame(flame: boolean): string[] {
  const body = [
    '       _./\\_.',
    '      /  ||  \\',
    '     |   ||   |',
    '     |   BE   |',
    '     |   ||   |',
    '      \\__||__/',
    flame ? '       /\\/\\/\\' : '        \\/\\/',
  ];
  return body;
}

// Starfield row of a given width — a sparse scattering of dim points.
function stars(width: number, seed: number): string {
  const cols = '·˖⋆✦';
  let out = '';
  for (let i = 0; i < width; i++) {
    const r = (Math.sin((i + 1) * (seed + 3)) + 1) / 2;
    out += r > 0.82 ? cols[Math.floor(r * cols.length) % cols.length] : ' ';
  }
  return out;
}

export interface LaunchOpts {
  paints: Paints;
  width?: number;
}

// Play the launch. Resolves when done. Renders ~22 frames then a LIFTOFF stamp.
export async function playLaunch(opts: LaunchOpts): Promise<void> {
  const { paints, width = 42 } = opts;
  const tty = process.stdout.isTTY;
  const skip = process.env.NO_ANIM === '1' || process.env.CI === '1';

  // Static frame for non-interactive use (piped/CI): one calm line, no cursor tricks.
  if (!tty || skip) {
    console.log(paints.blood('◈') + paints.dim(' Bebop ship powered up. Liftoff ready.'));
    return;
  }

  const FRAMES = 22;
  const rows = 9;
  const red = paints.blood;
  const amber = paints.amber;
  const hull = paints.hull;
  const dim = paints.dim;

  process.stdout.write(HIDE_CURSOR);
  try {
    for (let f = 0; f < FRAMES; f++) {
      // ship rises as the frame count climbs: higher f → ship printed lower with more space above
      const lift = Math.floor((f / FRAMES) * (rows - 1));
      const flame = f % 2 === 0;
      const lines: string[] = [];
      for (let r = 0; r < rows; r++) {
        if (r < lift) {
          lines.push(dim(stars(width, r + f)));
        } else if (r === lift) {
          // draw the ship body, centered
          const ship = shipFrame(flame);
          const s = ship[0];
          lines.push(' '.repeat(Math.floor((width - 12) / 2)) + red(s));
        } else if (r > lift && r - lift - 1 < 6) {
          const ship = shipFrame(flame);
          const s = ship[r - lift];
          const colored = s.includes('BE') ? red(s.replace('BE', amber('BE'))) : hull(s);
          lines.push(' '.repeat(Math.floor((width - 12) / 2)) + colored);
        } else {
          lines.push(dim(stars(width, r + f)));
        }
      }
      // exhaust flicker in amber/red under the ship
      if (flame) {
        lines.push(' '.repeat(Math.floor((width - 4) / 2)) + amber('≈≈≈'));
      } else {
        lines.push(' '.repeat(Math.floor((width - 4) / 2)) + red('≈≈'));
      }
      const frame = lines.join('\n');
      process.stdout.write(`${CLEAR_LINE}${frame}\r`);
      await sleep(70);
      // move cursor back up to overwrite the block next tick
      process.stdout.write(UP(rows + 1));
    }
    // final: clear + stamp
    process.stdout.write(`${CLEAR_LINE}${UP(rows + 1)}`);
    for (let i = 0; i < rows + 1; i++) process.stdout.write(`${CLEAR_LINE}\n`);
    process.stdout.write(UP(rows + 1));
    console.log(paints.blood('◈ LIFTOFF') + paints.dim(' — Bebop ship is away. Your kitchen, your ship, your cut.'));
  } finally {
    process.stdout.write(SHOW_CURSOR);
  }
}
