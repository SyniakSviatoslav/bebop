# Agent parity — borrowing the best of Claude Code & Hermes

This document records the publicly documented CLI/UX patterns from **Claude Code** and **Hermes**
that are worth integrating into Bebop, why each maps cleanly onto Bebop's existing architecture,
and what we shipped. "Reverse engineer" here means *study the public surface and borrow the
portable design ideas* — not reimplement proprietary internals. Bebop's guard OS, content-addressed
kernel, governor, and living memory are the substrate; the patterns below are natural fits.

## Source patterns (public)

| Pattern | Claude Code | Hermes | Bebop mapping |
| --- | --- | --- | --- |
| Slash commands | `/help`, `/clear`, `/model`, `/status`, `/plan`, `/compact`, `/resume` | skills-on-demand | `/`-dispatcher in `bebop.ts` |
| Hooks | PreToolUse/PostToolUse/Stop, deny decisions via `permissionDecision` | hooks staged/pushed | `src/hooks.ts` + guard gate |
| Permissions | `settings.json` `permissions.allow/deny` (glob rules) | red-line globs | user `~/.bebop/settings.json` `permissions` (project `bebop.json` is model-only) + `guard.ts` scope |
| Settings | `~/.claude/settings.json`, `.claude/settings.json` (project) | profile | `src/settings.ts` (project + user) |
| Plan mode | read-only; Explore/Plan subagents; no edits until approved | plan skill | `--plan` flag → Write/Edit denied loop |
| Headless | `claude -p "q"`, `--print`, `--json`, piped stdin | terminal tool | `bebop run -p/--json` one-shot |
| Subagents | `.claude/agents/*.md` (name/description/tools/model); read-only Explore, Plan | delegate_task | `subagent(task,{tools,model})` scoped loop |
| Skills | `SKILL.md` (frontmatter + body, `@`-mention/auto) | skills system | `src/skills.ts` loader (agent-skills fmt) |
| Model routing | `--model`, cost tiers | TOKEN ROUTER | already in `router.ts` |
| Memory | `CLAUDE.md` + auto memory | persistent memory | already in `memory.ts`/`knowledge.ts` |
| MCP | `claude mcp` | MCP tools | already in `src/mcp.ts` |

## Bounded integration (this pass)

1. **`src/settings.ts`** — load `bebop.json` from cwd (project, **untrusted — model only**) and
   `~/.bebop/settings.json` (user, trusted — may set `model` / `permissions` / `hooks`).
   A project `bebop.json` may set ONLY `model`; `permissions` and `hooks` are ignored + warned.
   The user file's `permissions.deny` globs feed the guard red-lines; `model` overrides default routing.
2. **`src/hooks.ts`** — a hooks runner with events `PreToolUse`, `PostToolUse`, `Stop`. Each hook is a
   command run with JSON on stdin; a `permissionDecision: "deny"` (exit-2 / stdout) blocks the action.
   Wired into `runLoop` so PreToolUse runs *before* the guard gate — native mapping of Claude's hook.
3. **Slash commands** in `bebop.ts`: `/help /status /route /clear /model /plan /resume /compact`.
   `/clear` resets the in-process living memory; `/plan` toggles plan mode; `/model` shows/routes.
4. **Plan mode + headless** in `bebop run`: `--plan` denies Write/Edit (Explore/Plan semantics);
   `-p "task"` / `--json` runs one shot and exits with structured output (no prompts).
5. **`subagent(task, {tools, model})`** — runs a scoped `runLoop` (read-only + cheaper model by
   default, returns only the summary) — the context-saving pattern from subagents.
6. **`src/skills.ts`** — load `SKILL.md` files (agent-skills frontmatter) from `.bebop/skills/`;
   `loadSkills()` + `findSkill(query)`. One sample skill shipped (`/review`).

## Explicitly out of scope (don't cargo-cult)

- Proprietary model behavior, remote-control/teleport, routines/cloud scheduling, gateway SSO,
  desktop app, channels (Slack/Discord) — these are hosted-platform features; Bebop is local-first.
- `bypassPermissions` / `--dangerously-skip-permissions` — antithetical to Bebop's guard OS. Not added.

## Verification

Every capability ships with a RED+GREEN test: settings deny overrides scope; a PreToolUse hook
can deny (RED) and allow (GREEN); plan mode blocks edits; headless emits JSON; subagent runs
read-only; skill loads. `tsc` clean; full suite green on both install paths.

## ▶ Live CLI

> Real `bebop` output, recorded with [asciinema](https://asciinema.org) → [agg](https://github.com/asciinema/agg) (no staging, no post-editing).

**bebop agents — parity with Claude Code / Hermes via adapters**

![bebop agents — parity with Claude Code / Hermes via adapters](../footage/feat-agents.gif)

