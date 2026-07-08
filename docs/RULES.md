# Constant Doubt — the universal verification rule for Bebop

> **No verification → no statement. Zero guesses. Every claim is falsifiable or it is removed.**

This is the load-bearing law of the Bebop project. It overrides convenience, optimism, and
"it probably works." It applies to **all** prose: README, every `docs/**` page, CHANGELOG,
code comments that describe behavior, and anything we tell a user.

## The rule, in one line

> A statement about Bebop is allowed to exist **only if** a real, reproducible probe or a
> deterministic test backs it. If you cannot make it go RED on bad input, it is not verified.

## What "verified" means

A claim is verified only when **one** of these holds, and the proof command is recorded next to
the claim:

1. **Live probe** — the actual `bebop` binary was run and produced the stated output. The exact
   command is pasted (e.g. `bebop dispatch "edit packages/db/migrations/x.sql"` → `⛔ DENIED`).
2. **Deterministic test** — a test in `npm test` (or `cargo test -p bebop-core`) asserts it, and
   that test has a RED case that flips it. A test that cannot fail is a *false-green metric* and
   does **not** verify anything.
3. **Source of truth** — the claim is a direct quote of code that is itself covered by (1) or (2).

## The three refusals

- **Refuse to state what you haven't run.** If a feature isn't executed, write "not yet verified"
  or nothing — never "works."
- **Refuse to guess at numbers.** Test counts, latencies, model names come from `npm test` output
  or live runs — never memory.
- **Refuse silent drift.** When code changes, the doc that describes it is updated in the same
  breath. A doc that lags the code is a lie.

## How to apply it (checklist before any commit touches docs)

- [ ] Every command named in a doc was actually invoked in this session.
- [ ] Every test file referenced (`*.test.ts`) exists and is in the green suite.
- [ ] Every number (test counts, model routing, globs) was read from live output or source.
- [ ] Every claim about security (what `bebop.json` / `~/.bebop/settings.json` may set) matches
      `src/settings.ts` — the untrusted-project / trusted-user split is law.
- [ ] The RED case ships beside the GREEN case.

## The standing posture: constant doubt

Treat every prior claim — including ones you wrote — as **suspect until re-probed**. The guard
kernel's own `selfTest()` is the model: certify by proving it denies the bad case, not by
asserting it permits the good one. Same for docs.

If you find a doc statement that does not survive a live probe, **fix the doc to match reality**
(or fix the code and re-probe). Never paper over a gap with a confident sentence.
