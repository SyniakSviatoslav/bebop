# Bebop — narration transcript

> Spoken version of the README for listeners (and a text transcript for readers).
> Tone: warm, plain, no jargon. Recorded from `docs/narration/README-narration.mp3`.

## Part 1 — The story

Bebop is your own coding helper that can drive any of the popular robot coders — Claude Code,
Codex, OpenCode, Hermes, Aider, Goose — from one steering wheel, behind one bodyguard.

Here is the five-year-old version. Imagine you have a robot that writes computer code for you. But
every robot has its own remote control and its own rules. If you switch robots, you have to learn a
whole new way to talk. Bebop is the one steering wheel that fits every robot. You sit in the Bebop
seat and tell it "go," and Bebop picks the right robot — usually the small, free one, unless the job
is hard.

Bebop also has a bodyguard. Before any robot touches your files, the bodyguard checks: is this the
money drawer, the secret drawer, the do-not-touch drawer? If yes, the robot is not allowed in unless
a grown-up says it is okay. The bodyguard never gets tired and never forgets. And the best robot is
free — Bebop starts on a no-cost model, so you can build before you pay a cent.

## Part 2 — Why businesses care

Most teams pick one coding robot and then get locked in: their scripts and safety rules only work
for that one. Switching later means rewriting everything. And the safety rails live inside the
vendor's black box, so you can't prove what the robot was allowed to do.

Bebop removes four pains. One: no lock-in — one control surface for every connected agent; switch
with a single command and your rules move with you. Two: a guard you can audit — a fail-closed,
Rust-powered bodyguard that denies money, secrets, and migrations unless a human approves, and a
self-test that proves the gates fire. Three: no cost surprise — free model by default, cheap lane for
the boring eighty percent. Four: no unverifiable claims — every sentence in the docs is backed by a
live test you can re-run yourself.

## Part 3 — Honest limits

Bebop is not magic. It is not a model provider — it drives models, it does not make them, so output
quality depends on the agent you point at. It is not a sandbox — the guard is a policy gate, not an
operating-system cage; run agents with least privilege. It is not a living-knowledge retriever yet —
recall is honest about that. It is not a graphical app — it is a terminal tool. It is not a
replacement for human review of money or auth changes; when a human approves, the human owns it. It
is not multi-user and not a hosted service — one person, one local process. And it does not guarantee
success — the copilot reduces risk, it does not make a robot all-knowing.

That is Bebop: one wheel, one bodyguard, free to start, and honest about what it is not.
