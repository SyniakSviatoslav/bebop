# Live footage — how it's made (and how to reproduce)

The GIF at the top of the README (`bebop-session.gif`) is a **real recording** of the Bebop CLI, not
a mock-up. It is produced by:

1. **Recording** a live session with [asciinema](https://asciinema.org):
   ```bash
   NO_ANIM=1 asciinema rec -t "Bebop — live session" \
     -c "bash scripts/_rec_session_inner.sh" docs/footage/bebop-session.cast --overwrite
   ```
   (driven by `scripts/record-session.sh`; the inner script runs genuine `bebop boot`,
   `bebop status`, `bebop use native`, `bebop dispatch`, `bebop route`, `bebop map`).

2. **Converting** the cast to a GIF with [agg](https://github.com/asciinema/agg) (the official
   asciinema GIF generator):
   ```bash
   agg --theme nord --speed 1.15 --cols 100 --rows 26 \
     docs/footage/bebop-session.cast docs/footage/bebop-session.gif
   ```

## Files
- `bebop-session.cast` — the raw asciinema v2 recording (replayable with `asciinema play`).
- `bebop-session.gif` — the rendered GIF embedded in the README.

## Verify it's real
Replay the cast: `asciinema play docs/footage/bebop-session.cast`. Every frame is the actual stdout
of the `bebop` binary on this repo. Re-run `scripts/record-session.sh` to regenerate from the
current code — the output will match unless the CLI changed.
