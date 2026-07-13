# bebop host-agent — criterion baseline (FIRST CAPTURE)

Machine: Linux 6.8.0-124-generic. Captured 2026-07-13.
Run: `cargo bench --bench criterion -- --warm-up-time 1 --measurement-time 3 --sample-size 10`

| benchmark            | mean (µs) | low (µs) | high (µs) |
|----------------------|-----------|----------|-----------|
| loop_cycle/benign    | 460.9     | 456.9    | 463.4     |
| wire/benign          | 240.7     | 237.3    | 247.9     |

Notes:
- `loop_cycle` runs the full 6-layer control-loop state machine (INTAKE→…→DELIVER)
  over a benign "write the docs" task with a stable L5 proposal.
- `wire` runs the 3-layer wire (field sim + L5 stabilizer + gating) standalone.
- Tracing spans (`loop_cycle`, `wire`) are emitted; set RUST_LOG=debug to observe.
- Re-run after any loop/wire change to detect regressions.
