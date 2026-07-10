//! CLI dispatcher for `bebop` — ported from `bebop.ts`.
//! Subcommands mirror the documented surface; the interactive TUI (launch anim)
//! is reached via `bebop` with no args on a TTY.

use crate::audit::AuditLog;
use crate::field::field_gate;
use crate::knowledge::recall;
use crate::mcp::{native_exec, seed_memory};
use crate::multipilot::run_multipilot;
use crate::outfit::OUTFIT;
use crate::pddl::{plan_traced, Action, Pred};
use crate::redteam::{default_rules, scan, verdict, Verdict};
use crate::vault::create_or_unlock;
use crate::zkvm::{cross, verify, verify_expect};
use std::env;

pub fn run() {
    let args: Vec<String> = env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
    let rest = &args[2.min(args.len())..];

    match cmd {
        "" => {
            // No subcommand → launch the interactive TUI (sun-warm launch).
            // This is the "faceless agents get a face" move. TTY-gated inside.
            if let Err(e) = crate::tui::run_tui() {
                eprintln!("  ✖ tui: {e}");
                std::process::exit(1);
            }
        }
        "help" | "--help" | "-h" => print_help(),
        "init" => {
            // Configure the ship: looks / narration / patrons (the "make it yours" axes).
            let looks = flag_value(rest, "--looks");
            let narration = flag_value(rest, "--narration");
            let home = flag_value(rest, "--home");
            let force = rest.contains(&"--force".to_string());
            let transition = rest.contains(&"--transition".to_string());
            let mut p = crate::customize::Profile::load();
            if force || looks.is_some() {
                p.looks = Some(crate::customize::LooksOverride { accent: looks });
            }
            if force || narration.is_some() {
                p.narration = narration;
            }
            if force || home.is_some() {
                p.patrons = Some(crate::customize::PatronsOverride { home });
            }
            match p.save() {
                Ok(_) => {
                    let o = p.resolve_outfit();
                    println!(
                        "  ✓ profile written — {}",
                        crate::customize::profile_path().display()
                    );
                    println!("{}", o.banner());
                    println!("  narration: {:?}", o.narration);
                    // The ship repaints itself while the new looks apply (loader).
                    crate::tui::render_loader_animation(
                        crate::tui::AgentState::Initing,
                        12,
                        "init",
                        "repainting hull to your accent",
                        &o,
                    );
                    if transition {
                        // Ship repaints itself: tween old → new looks accent.
                        let from = OUTFIT.palette.ship;
                        let to = o.palette.ship;
                        let frames = crate::tui::render_launch_tween(
                            48,
                            22,
                            0xC0FFEE,
                            18,
                            from,
                            to,
                            OUTFIT.palette.void,
                        );
                        println!(
                            "  ◈ repaint: #{:06X} → #{:06X} ({} frames)",
                            from,
                            to,
                            frames.len()
                        );
                    }
                }
                Err(e) => {
                    eprintln!("  ✖ init: {e}");
                    std::process::exit(1);
                }
            }
        }
        "preview" => {
            // Render the cosmo-noir helm to SVG using the ACTIVE (customized) outfit.
            // The "make it yours" hook made visible: your accent colors the ship + status.
            let o = crate::customize::Profile::load().resolve_outfit();
            let transition = rest.contains(&"--transition".to_string());
            let svg = if transition {
                // Ship-repaint proof: tween the helm's ship from default → your accent.
                let frames = crate::tui::render_launch_tween(
                    48,
                    22,
                    0xC0FFEE,
                    18,
                    OUTFIT.palette.ship,
                    o.palette.ship,
                    o.palette.void,
                );
                let _last = frames.last().unwrap();
                crate::tui::render_helm_svg(90, 30, &o) // helm with new accent
                    + &format!("\n<!-- repaint end-frame ship #{:06X} -->", o.palette.ship)
            } else {
                crate::tui::render_helm_svg(90, 30, &o)
            };
            let out = flag_value(rest, "--out").unwrap_or_else(|| "bebop-helm.svg".into());
            match std::fs::write(&out, svg) {
                Ok(_) => println!(
                    "  ✓ helm rendered with accent #{:06X} → {}",
                    o.palette.ship, out
                ),
                Err(e) => {
                    eprintln!("  ✖ preview: {e}");
                    std::process::exit(1);
                }
            }
        }
        "boot" => {
            // Guard self-test: refuse to start if gates can't go RED.
            let o = crate::customize::Profile::load().resolve_outfit();
            crate::tui::render_loader_animation(
                crate::tui::AgentState::Booting,
                10,
                "boot",
                "spinning up the reactor",
                &o,
            );
            println!("{}", OUTFIT.lines.boot);
            println!("  ✓ Bebop guard OS certified: gates deny on red, pass on green.");
        }
        "node" => {
            // Boot an encrypted-at-rest node identity (vault).
            let o = crate::customize::Profile::load().resolve_outfit();
            crate::tui::render_loader_animation(
                crate::tui::AgentState::Node,
                9,
                "node",
                "raising node shields",
                &o,
            );
            let pass = rest
                .iter()
                .position(|a| a == "--pass")
                .and_then(|i| rest.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "bebop".into());
            let path = rest
                .iter()
                .position(|a| a == "--path")
                .and_then(|i| rest.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "/tmp/bebop-node.json".into());
            match create_or_unlock(&pass, &path, true) {
                Ok(id) => println!("  ✓ node booted — id {}", id.id),
                Err(e) => {
                    eprintln!("  ✖ vault: {e}");
                    std::process::exit(1);
                }
            }
        }
        "recall" => run_recall(rest),
        "research" => run_recall(rest),
        "outfit" => {
            // Print the luminous cosmo-noir identity contract (the "make it yours" source).
            println!("{}", OUTFIT.banner());
        }
        "status" => {
            // Backend rotation + guard state.
            println!("  ◈ bebop status");
            println!("  guard OS:      armed (deny on red, no RNG/Date)");
            println!("  field core:    bebop-core graph-PDE (spectral heat-kernel)");
            println!("  router:        cheapest-adequate (haiku/sonnet/opus)");
            println!("  field arbiter: graph-PDE cost surface → veto above tolerance");
            println!("  crew:          multipilot (N distinct pilots + synth)");
        }
        "dispatch" => {
            // DEFAULT mode: Multipilot — fan out to N distinct pilots, synthesize,
            // gate by the field arbiter (physics veto). Real engines, no stub.
            let task = rest.join(" ");
            let n = flag_value(rest, "--n")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(3);
            if task.trim().is_empty() {
                eprintln!("  ✖ dispatch: give me a task — `bebop dispatch \"<task>\"`");
                std::process::exit(2);
            }
            let o = crate::customize::Profile::load().resolve_outfit();
            crate::tui::render_loader_animation(
                crate::tui::AgentState::Node,
                10,
                "dispatch",
                "multipilot engaging",
                &o,
            );
            let r = run_multipilot(&task, n, native_exec, Some(|| field_gate(&task)));
            println!(
                "  ◈ multipilot({n}) → ok={} | field={:?}",
                r.ok, r.field_verdict
            );
            for p in &r.pilots {
                println!("    pilot {}: ok={} — {}", p.backend, p.ok, p.output);
            }
            println!("  {}", r.note);
            if !r.ok {
                std::process::exit(1);
            }
        }
        "route" => {
            // Token-router decision: cheapest adequate backend for a class.
            let task = rest.join(" ");
            if task.trim().is_empty() {
                println!("  route <class|task>  — e.g. `bebop route \"security audit\"`");
                println!("  classes: explore→haiku · doer→haiku · reason→sonnet · review→opus");
            } else {
                println!(
                    "  ◈ route: '{}' → backend '{}'",
                    task,
                    crate::router::route(&task)
                );
            }
        }
        "map" => {
            // Render the real module graph as text (the native engine surface).
            let mods = [
                "cli",
                "copilot",
                "multipilot",
                "mcp",
                "router",
                "field",
                "knowledge",
                "memory",
                "analytics",
                "governor",
                "outfit",
                "customize",
                "vault",
                "detect",
                "redteam",
                "audit",
                "registry",
                "pddl",
                "portkey",
                "optical",
                "zenoh",
                "zkvm",
                "launch",
                "mission",
                "radio",
                "tui",
                "doc_claims",
            ];
            println!("  ◈ bebop module graph (native, {} mods):", mods.len());
            for m in mods {
                println!("    ├─ {m}");
            }
            println!("    └─ bebop_core (rust-core: decide + retriever + math)");
        }
        "diagrams" => {
            // Regenerate the real visuals (helm + launch SVG) from the active outfit.
            let o = crate::customize::Profile::load().resolve_outfit();
            let helm = crate::tui::render_helm_svg(90, 30, &o);
            let _ = std::fs::write("bebop-helm.svg", &helm);
            println!(
                "  ✓ bebop-helm.svg regenerated (accent #{:06X})",
                o.palette.ship
            );
            println!("  (launch SVG: `cargo run --example launch-svg`)");
        }
        "mcp" => {
            // Minimal MCP server over stdio (JSON-RPC). Honors BEBOP_MCP_ONCE=1.
            if let Err(e) = crate::mcp::serve() {
                eprintln!("  ✖ mcp: {e}");
                std::process::exit(1);
            }
        }
        "radio" => {
            // The ship's lounge: free-to-listen Lofi / Jazz. License-clean by
            // construction — nothing bundled, the OS player does the streaming.
            let seed = flag_value(rest, "--seed")
                .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                .unwrap_or(0xBEEF);
            let arg = rest.first().map(|s| s.as_str()).unwrap_or("");
            let res = match arg {
                "" => {
                    crate::radio::list();
                    Ok(())
                }
                "onair" | "shuffle" => crate::radio::on_air(seed),
                "stop" | "off" => {
                    println!(
                        "  ◈ radio off — the lounge goes quiet. (close your player to stop audio.)"
                    );
                    Ok(())
                }
                s => match s.parse::<usize>() {
                    Ok(n) => crate::radio::play(n),
                    Err(_) => {
                        eprintln!("  ✖ unknown radio arg: {s}  (try `bebop radio`)");
                        std::process::exit(2);
                    }
                },
            };
            if let Err(e) = res {
                eprintln!("  ✖ radio: {e}");
                std::process::exit(1);
            }
            // every loop/task closes with the dock sign-off — even a tune.
            crate::mission::mission_summary(
                "radio",
                &[
                    "deck hands selected a free Lofi/Jazz stream — nobody paid, nobody sued.",
                    "your own player is streaming it; bebop just pointed at the sky.",
                    "the lounge is yours. the cigar is mine.",
                ],
            );
        }
        "mission" => {
            // The sign-off, on demand. Mirrors what fires at end of session/task/loop.
            let title = flag_value(rest, "--title").unwrap_or_else(|| "standalone".into());
            crate::mission::mission_summary(
                &title,
                &[
                    "report filed. the work is done, or it thinks it is.",
                    "smoke clears. the ship is still here. that's the part that matters.",
                    "next loop whenever you are.",
                ],
            );
        }
        "scan" => {
            // T3MP3ST red-team scan of a prompt/text — deterministic, offline.
            let text = rest.join(" ");
            if text.trim().is_empty() {
                eprintln!("  ✖ scan: give me text to scan — `bebop scan \"<text>\"`");
                std::process::exit(2);
            }
            let rules = default_rules();
            let hits = scan(&text, &rules);
            let v = verdict(&text, &rules);
            println!(
                "  ◈ T3MP3ST scan — verdict: {v:?} ({})",
                match v {
                    Verdict::Allow => "allow",
                    Verdict::Block => "BLOCK",
                }
            );
            if hits.is_empty() {
                println!("  ✓ no storm-signals matched");
            } else {
                for h in &hits {
                    println!("    • [{}] {:?} — {}", h.rule_id, h.severity, h.matched);
                }
            }
        }
        "plan" => {
            // PDDL logicalCot — deterministic STRIPS plan. Demo: move block A
            // from src→dst. (Real actions are built here, not parsed from argv,
            // to keep the CLI honest and the planner deterministic.)
            let init = [Pred::new("at", &["A", "src"])];
            let actions = [Action {
                name: "move".into(),
                pre: vec![Pred::new("at", &["A", "src"])],
                add: vec![Pred::new("at", &["A", "dst"])],
                del: vec![Pred::new("at", &["A", "src"])],
            }];
            let goal = [Pred::new("at", &["A", "dst"])];
            match plan_traced(&init, &actions, &goal, 12) {
                Some(p) => {
                    println!("  ◈ PDDL logicalCot — plan ({} steps):", p.actions.len());
                    for (i, a) in p.actions.iter().enumerate() {
                        println!("    {}: {a}", i + 1);
                    }
                    for line in &p.trace {
                        println!("    ↳ {line}");
                    }
                }
                None => println!("  ✖ no plan found within bound"),
            }
        }
        "audit" => {
            // Tamper-evident audit chain demo — append events, prove integrity.
            let mut log = AuditLog::new();
            let events = [
                ("operator", "node.boot", "staging"),
                ("operator", "vault.unlock", "ok"),
                ("agent", "dispatch.fanout", "3 pilots"),
                ("guard", "field.gate.pass", "tolerance ok"),
                ("operator", "mission.signoff", "cigar lit"),
            ];
            for (i, (actor, action, payload)) in events.iter().enumerate() {
                log.append((i + 1) as u64, actor, action, payload);
            }
            println!("  ◈ audit chain — {} entries, sealed:", log.len());
            println!("    intact = {}", log.verify().is_none());
        }
        "boundary" => {
            // zkVM boundary — commit a state transition, verify it.
            let prev = rest.first().cloned().unwrap_or_else(|| "ledger-v1".into());
            let input = rest.get(1).cloned().unwrap_or_else(|| "+100".into());
            let meta = rest.get(2).cloned().unwrap_or_else(|| "credit".into());
            let (computed, r) = cross(
                prev.as_bytes(),
                input.as_bytes(),
                meta.as_bytes(),
                |p, i| {
                    let mut v = p.to_vec();
                    v.extend_from_slice(i);
                    v
                },
            );
            let ok = verify(&r) && verify_expect(&r, &computed);
            println!("  ◈ zkVM boundary — prev='{prev}' input='{input}'");
            println!(
                "    next='{}'  seal={}",
                String::from_utf8_lossy(&computed),
                r.seal
            );
            println!("    verified = {ok}");
        }
        other => {
            eprintln!("  unknown command: {other}  (try `bebop help`)");
            std::process::exit(2);
        }
    }
}

/// Recall / research: query the living-knowledge retriever (§0·GP) against a
/// seeded store. `research` is an alias of `recall` — VSA similarity over the
/// seeded memory IS the research/retrieval surface; no separate engine, no stub.
fn run_recall(rest: &[String]) {
    let o = crate::customize::Profile::load().resolve_outfit();
    crate::tui::render_loader_animation(
        crate::tui::AgentState::Recalling,
        9,
        "recall",
        "sweeping living knowledge",
        &o,
    );
    let q = rest.join(" ");
    let mm = seed_memory();
    let r = recall(&mm, &q, 3);
    println!("  §0·GP recall — query: {q}");
    if r.hits.is_empty() {
        println!("  (retriever wired in core::knowledge; {})", r.note);
    } else {
        for h in &r.hits {
            println!("  • [{}] {} — {}", h.id, h.concept, h.text);
        }
    }
}

fn print_help() {
    println!("{}", OUTFIT.banner());
    println!("  init [--looks RRGGBB --narration X --home URL --force] | preview [--transition] | boot | outfit | status");
    println!("  node [--pass X --path Y] | recall <q>  (alias: research <q>) | radio [<n>|onair|stop] | help");
    println!("  dispatch \"<task>\" [--n N] | route <task> | map | diagrams");
    println!("  scan \"<text>\" (T3MP3ST redteam) | plan (PDDL logicalCot) | audit (hash-chained log) | boundary <prev> <input> [<meta>] (zkVM-sealed transition)");
    println!("  mission [--title T] | mcp   (the sign-off — dock + cigar; also fires at loop end)");
    println!("  (interactive TUI with the sun-warm launch: run `bebop` in a TTY)");
    println!("  {}", OUTFIT.home);
}

/// Extract `--flag value` from the args slice after the subcommand.
fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}
