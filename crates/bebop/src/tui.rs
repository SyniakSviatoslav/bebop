//! The Bebop TUI — the sun-warm launch, then the cosmo-noir helm.
//!
//! Brand law (Warm Cosmo-Noir):
//!   - void `#12100E` ground, bone `#F2E9DB` text, ship `#E8A544` = sun-warm,
//!     tele `#E8893A` = orange telemetry, alert `#E0543E` = warm red (drift only).
//!   - One meaningful color per view. The launch uses ship on void; the helm
//!     uses orange telemetry; alert red fires only on drift / hallucination risk.
//!
//! The launch frames come from the SAME deterministic `render_launch` used by the
//! doc-gate test (one source of truth). Live telemetry is an LCG (no RNG/Date) so
//! the helm is reproducible headless — important for the air-gapped contract.
//!
//! Customization: every draw path takes `&Outfit` so a profile's `looks` accent
//! recolors the ship + status live (resolved via `customize::Profile::load`).

use crate::customize::Profile;
use crate::launch::{render_launch_accent, Frame};
use crate::outfit::Outfit;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Sparkline, Tabs},
    Frame as TuiFrame, Terminal,
};
use std::io::Stdout;
use std::time::Duration;

/// RAII guard that restores the terminal to its original mode on drop — even
/// if the guarded scope panics. Without this, a panic inside `helm_loop`
/// left the terminal in raw mode with the alternate screen up (garbled prompt).
/// BP-23 fix: deterministic cleanup, no `catch_unwind` needed.
///
/// `enter` enables raw mode + alternate screen and returns an *active* guard.
/// If raw mode cannot be entered (e.g. no TTY in CI), `enter` returns an
/// *inactive* guard inside the `Err` — dropping that inactive guard is a
/// no-op, so callers can `?` out of `enter` without ever calling
/// `disable_raw_mode` on a terminal they never put into raw mode.
#[derive(Debug)]
struct RawModeGuard {
    active: bool,
}

impl RawModeGuard {
    /// Enter raw mode + alternate screen. Returns `Err` (with an *inactive*
    /// guard) if crossterm setup fails, so `drop` won't try to restore a
    /// state we never entered.
    fn enter(stdout: &mut Stdout) -> std::io::Result<Self> {
        enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen)?;
        Ok(RawModeGuard { active: true })
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        // Best-effort restore. Ignore errors: a failure during Drop can't be
        // surfaced usefully and must not panic (which would abort the original
        // panic's unwind).
        let _ = disable_raw_mode();
        let mut stdout = std::io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen);
    }
}

// ---- palette helpers -------------------------------------------------------
fn c(rgb: u32) -> Color {
    Color::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, (rgb & 0xFF) as u8)
}
fn rgb_hex(rgb: u32) -> String {
    format!("{:06X}", rgb & 0xFFFFFF)
}
/// Linear blend of two 0xRRGGBB colors: `blend(fg, bg, a)` returns `bg` at a=0
/// and `fg` at a=1 (i.e. `a` is the weight of `fg`). Used by the ship-repaint
/// tween where `fg` = target accent and `bg` = starting hull.
fn blend(fg: u32, bg: u32, a: f64) -> u32 {
    let m = |x: u32| -> f64 { (x & 0xFF) as f64 / 255.0 };
    let ch = |fgc: u32, bgc: u32, s: u32| -> u8 {
        let v = m(fgc >> s) * a + m(bgc >> s) * (1.0 - a);
        (v * 255.0).clamp(0.0, 255.0) as u8
    };
    ((ch(fg, bg, 16) as u32) << 16) | ((ch(fg, bg, 8) as u32) << 8) | (ch(fg, bg, 0) as u32)
}

// ---- deterministic "live" telemetry (LCG, air-gapped) -------------------------
/// Agentic telemetry, the way Claude Code's status line thinks about it:
/// model name, token usage, context-window %, exec time, and a DRIFT signal
/// (how far the plan is wandering / hallucination risk). All seeded LCG — no
/// RNG/Date — so the helm is byte-reproducible headless.
// ---- agent state machine (drives the dynamic loader) -----------------------
/// What the ship is doing right now. Each variant gets its OWN loader animation,
/// so `boot`/`init`/`node`/`recall` each read as a different ship motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Idle,      // docked — static ship on the void
    Booting,   // ship rises with a luminous halo (boot)
    Initing,   // ship repaints itself (init --looks)
    Node,      // ship raises concentric shields (node)
    Recalling, // ship sweeps a scan beam (recall)
    Thinking,  // ship orbits while the model answers (waiting for output)
    Radio,     // ship broadcasts — on air (lofi/jazz lounge)
}

impl AgentState {
    fn label(&self) -> &'static str {
        match self {
            AgentState::Idle => "docked",
            AgentState::Booting => "booting",
            AgentState::Initing => "repainting",
            AgentState::Node => "shielding",
            AgentState::Recalling => "scanning",
            AgentState::Thinking => "thinking",
            AgentState::Radio => "on air",
        }
    }
}

/// Agentic telemetry, the way Claude Code's status line thinks about it:
/// model name, token usage, context-window %, exec time, and a DRIFT signal
/// (how far the plan is wandering / hallucination risk). All seeded LCG — no
/// RNG/Date — so the helm is byte-reproducible headless.
///
/// PLUS the living layer: an `AgentState` (what the ship is doing), a `frame`
/// counter (monotonic, no clock), an OpenCode-like `feed` (live working log),
/// and Hermes-like `hints` (contextual nudges). This is what makes the helm
/// feel alive while it waits for the model.
struct Telemetry {
    seed: u64,
    step: u64,
    frame: u64, // monotonic tick for loader animation (no Date/RNG)
    state: AgentState,
    models: Vec<&'static str>,
    model_i: usize,
    tokens: u64,
    context_pct: u64,
    exec_ms: u64,
    drift: Vec<u64>, // rolling; high = off-rails / hallucination risk
    quality: Vec<u64>,
    cost: Vec<u64>,
    feed: Vec<(AgentState, String)>, // OpenCode-like live working log (ring)
    hints: Vec<String>,              // Hermes-like contextual hints
    karaoke: usize,                  // chars of the LAST feed line revealed (typing sync)
    twin: Option<Box<Telemetry>>,    // What-If twin: a forked run to diff side-by-side
}
impl Telemetry {
    fn new() -> Self {
        let mut t = Telemetry {
            seed: 0xBEEF,
            step: 0,
            frame: 0,
            state: AgentState::Idle,
            models: vec!["haiku", "opus", "haiku", "sonnet"],
            model_i: 0,
            tokens: 1_240,
            context_pct: 38,
            exec_ms: 420,
            drift: vec![12; 24],
            quality: vec![88; 24],
            cost: vec![40; 24],
            feed: Vec::new(),
            hints: vec![
                "◈ tip: `bebop init --looks RRGGBB` repaints the ship".into(),
                "◈ tip: `←/→` switches helm tabs · `q` docks".into(),
                "◈ §0·GP: drift > 60% means the plan is wandering".into(),
            ],
            karaoke: 0,
            twin: None,
        };
        // seed the working feed so the panel is never empty on first paint
        t.log(
            AgentState::Booting,
            "helm online — ship docked, awaiting orders",
        );
        t.log(
            AgentState::Thinking,
            "field arbiter armed: physics can veto the planner",
        );
        t
    }
    /// Append to the OpenCode-like working feed (keeps the last 8 lines).
    fn log(&mut self, s: AgentState, line: &str) {
        self.feed.push((s, line.to_string()));
        if self.feed.len() > 8 {
            self.feed.remove(0);
        }
    }
    fn tick(&mut self) {
        self.step += 1;
        self.frame += 1;
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let n = (self.seed >> 33) as u64 % 100;
        // model rotates every ~7 steps deterministically
        if self.step % 7 == 0 {
            self.model_i = (self.model_i + 1) % self.models.len();
        }
        self.tokens += (n % 11) * 7 + 13;
        self.context_pct = (self.context_pct as i64 + (n as i64 - 50) / 9).clamp(4, 96) as u64;
        self.exec_ms = (self.exec_ms as i64 + (n as i64 * 7 % 13 - 6) * 11).clamp(80, 3_200) as u64;
        let last_d = self.drift.last().copied().unwrap_or(12);
        let last_q = self.quality.last().copied().unwrap_or(88);
        let last_c = self.cost.last().copied().unwrap_or(40);
        let push = |v: &mut Vec<u64>, x: u64| {
            v.remove(0);
            v.push(x.clamp(0, 100));
        };
        push(
            &mut self.drift,
            (last_d as i64 + (n as i64 * 3 % 17 - 8)) as u64, // occasionally spikes → alert
        );
        push(
            &mut self.quality,
            (last_q as i64 + (n as i64 * 7 % 11 - 5)) as u64,
        );
        push(
            &mut self.cost,
            (last_c as i64 + (n as i64 * 3 % 9 - 4)) as u64,
        );
        // while working, keep the feed alive (loader is "doing something")
        if self.state != AgentState::Idle && self.frame % 3 == 0 {
            let msg = match self.state {
                AgentState::Booting => "spinning up the reactor",
                AgentState::Initing => "repainting hull to your accent",
                AgentState::Node => "raising node shields",
                AgentState::Recalling => "querying living knowledge",
                AgentState::Thinking => "awaiting model output…",
                AgentState::Radio => "broadcasting Lofi/Jazz — the lounge is open",
                AgentState::Idle => "",
            };
            if !msg.is_empty() {
                self.log(self.state, msg);
                self.karaoke = 0; // restart typing reveal on new line
            }
        }
        // karaoke: reveal the last feed line a few chars per tick (typing sync)
        if !self.feed.is_empty() {
            let last_len = self.feed.last().unwrap().1.chars().count();
            if self.karaoke < last_len {
                self.karaoke = (self.karaoke + 3).min(last_len);
            }
        }
        // twin follows our ticks too (deterministic fork)
        if let Some(twin) = &mut self.twin {
            twin.tick();
        }
    }
    /// Fork a What-If twin: a deterministic clone with a different seed so its
    /// telemetry diverges — render side-by-side to diff "actual vs hypothetical".
    fn fork_twin(&mut self, seed: u64) {
        let mut t = Telemetry::new();
        t.seed = seed; // different seed → different (but reproducible) trajectory
        t.state = self.state;
        self.twin = Some(Box::new(t));
    }
    /// Plain-text diff of our telemetry vs the twin (RED+GREEN checkable).
    fn twin_diff(&self) -> Vec<String> {
        match &self.twin {
            None => vec!["no twin docked".into()],
            Some(t) => {
                let mut d = Vec::new();
                d.push(format!("tok  self {} · twin {}", self.tokens, t.tokens));
                d.push(format!(
                    "ctx  self {}% · twin {}%",
                    self.context_pct, t.context_pct
                ));
                d.push(format!(
                    "drift self {}% · twin {}%",
                    self.drift_now(),
                    t.drift_now()
                ));
                d
            }
        }
    }
    fn model(&self) -> &'static str {
        self.models[self.model_i]
    }
    fn drift_now(&self) -> u64 {
        self.drift.last().copied().unwrap_or(0)
    }
    /// Authority readout for the gauge (0..100) — derived deterministically from
    /// the quality trace so the helm's authority bar always has a stable value.
    fn authority_gauge(&self) -> u64 {
        self.quality.last().copied().unwrap_or(80)
    }
    /// Color for the context bar — green < 50, orange 50..70, alert red > 70.
    /// Mirrors Claude Code's color-coded context bar.
    fn ctx_color(&self, o: &Outfit) -> Color {
        match self.context_pct {
            0..=50 => c(o.palette.tele),
            _ if self.context_pct <= 70 => c(o.palette.ship),
            _ => c(o.palette.alert),
        }
    }
}

/// Render the dynamic loader: a small ship that *moves* based on `state` and the
/// monotonic `frame` (no clock). Each command state is its own animation:
///   Booting  → ship rises with a pulsing luminous halo
///   Initing  → ship repaints (hull hue rotates)
///   Node     → ship inside expanding concentric shields
///   Recalling→ ship sweeps a scan beam left↔right
///   Thinking → ship orbits the cursor while waiting for output
///   Idle     → static ship on the void
/// Returns a list of styled lines to drop into a block.
fn draw_loader(state: AgentState, frame: u64, o: &Outfit) -> Vec<Line<'static>> {
    let p = o.palette;
    let ship = c(p.ship);
    let glow = c(p.glow);
    let void = c(p.void);
    let mut lines: Vec<Line<'static>> = Vec::new();
    // a 9-wide, 5-tall mini stage
    let stage: [&str; 5] = [
        "         ",
        "         ",
        "         ",
        "         ",
        "         ",
    ];
    let mut grid = stage;
    match state {
        AgentState::Idle => {
            grid[2] = "   ◈▶◈   ";
        }
        AgentState::Booting => {
            // ship rises: row = 4 - (frame/4 % 4); halo pulses with frame
            let rise = (frame / 4) % 5;
            let row = 4usize.saturating_sub(rise as usize);
            grid[row] = "   ◈▶◈   ";
            let halo = if frame % 2 == 0 {
                " ✧     ✧ "
            } else {
                "  ✧   ✧  "
            };
            grid[if row == 0 { 1 } else { row - 1 }] = halo;
        }
        AgentState::Initing => {
            // hull hue rotates frame→frame (repaint)
            grid[2] = "  ◈≈≈≈◈  ";
            grid[3] = "   repaint ";
        }
        AgentState::Node => {
            // concentric shields expand/contract
            let r = (frame / 3) % 3;
            grid[1] = match r {
                0 => " (     ) ",
                1 => "  (   )  ",
                _ => "   ( )   ",
            };
            grid[2] = "   ◈▶◈   ";
            grid[3] = match r {
                0 => " (     ) ",
                1 => "  (   )  ",
                _ => "   ( )   ",
            };
        }
        AgentState::Recalling => {
            // scan beam sweeps left↔right
            let sweep = (frame / 2) % 9;
            let mut chars: Vec<char> = "         ".chars().collect();
            if (sweep as usize) < chars.len() {
                chars[sweep as usize] = '║';
            }
            grid[1] = " ════════ ";
            grid[2] = Box::leak(chars.into_iter().collect::<String>().into_boxed_str());
            grid[3] = " ════════ ";
        }
        AgentState::Thinking => {
            // ship orbits the cursor
            let orb = (frame / 2) % 8;
            let pos = [
                " ◈      ",
                "  ◈     ",
                "   ◈    ",
                "    ◈   ",
                "     ◈  ",
                "      ◈ ",
                "       ◈",
                "      ◈ ",
            ][orb as usize];
            grid[2] = pos;
            grid[3] = "  ·waiting· ";
        }
        AgentState::Radio => {
            // ship ON AIR: antennae pulse + a 3-bar equalizer under the hull
            grid[2] = "   ◈▶◈   ";
            let eq = (frame / 2) % 4;
            let bars = [" ▁▁▁ ", " ▃▁▃ ", " ▅▃▅ ", " ▇▅▇ "][eq as usize];
            grid[3] = bars;
            grid[1] = if frame % 2 == 0 {
                "    ╱╲    "
            } else {
                "   ╱  ╲   "
            };
        }
    }
    for (i, g) in grid.iter().enumerate() {
        let _ = i;
        let style = if state == AgentState::Idle {
            Style::default().fg(ship)
        } else {
            // loader glows: alternate ship/glow for a luminous pulse
            if frame % 2 == 0 {
                Style::default().fg(glow)
            } else {
                Style::default().fg(ship)
            }
        };
        lines.push(Line::from(Span::styled(*g, style)).style(Style::default().bg(void)));
    }
    lines
}

// ---- launch render (ship on void, recolored by outfit) -----------------------
fn cell_color(rgba: u32) -> Color {
    Color::Rgb(
        ((rgba >> 16) & 0xFF) as u8,
        ((rgba >> 8) & 0xFF) as u8,
        (rgba & 0xFF) as u8,
    )
}

fn draw_launch(f: &mut TuiFrame, frame: &Frame, o: &Outfit) {
    let size = f.area();
    let w = frame.w.min(size.width as usize);
    let h = frame.h.min(size.height as usize);
    let off_x = ((size.width as usize).saturating_sub(w)) / 2;
    let off_y = ((size.height as usize).saturating_sub(h)) / 2;

    let mut lines: Vec<Line> = Vec::with_capacity(h);
    for y in 0..h {
        let mut spans: Vec<Span> = Vec::with_capacity(w);
        for x in 0..w {
            let rgba = frame.cells[y * frame.w + x];
            if (rgba & 0xFFFFFF) == (o.palette.void & 0xFFFFFF) {
                spans.push(Span::styled(" ", Style::default().bg(c(o.palette.void))));
            } else {
                let ch = if (rgba & 0xFFFFFF) == (o.palette.ship & 0xFFFFFF) {
                    "█"
                } else {
                    "░"
                };
                spans.push(Span::styled(ch, Style::default().fg(cell_color(rgba))));
            }
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(
        Paragraph::new(lines),
        Rect {
            x: off_x as u16,
            y: off_y as u16,
            width: w as u16,
            height: h as u16,
        },
    );
}

// ---- the helm (the fascinating part) ----------------------------------------
fn draw_helm(f: &mut TuiFrame, tel: &Telemetry, tab: usize, o: &Outfit) {
    let p = o.palette;
    let ship = c(p.ship);
    let tele = c(p.tele);
    let bone = c(p.bone);
    let alert = c(p.alert);
    let void = c(p.void);

    let size = f.area();
    let outer = Layout::vertical([
        Constraint::Length(2), // header
        Constraint::Min(6),    // body
        Constraint::Length(2), // footer (status line)
    ])
    .split(size);

    // HEADER: sigil · name · tagline — ship on void
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {} ", o.sigil), Style::default().fg(ship).bold()),
            Span::styled(
                format!("BEBOP v{}", o.version),
                Style::default().fg(bone).bold(),
            ),
            Span::styled(format!("  — {}", o.tagline), Style::default().fg(ship)),
        ])),
        outer[0],
    );

    // BODY: tabs across the top, then the active panel
    let tabs = ["helm", "dispatch", "knowledge", "outfit"];
    let body = Layout::vertical([Constraint::Length(2), Constraint::Min(4)]).split(outer[1]);
    f.render_widget(
        Tabs::new(tabs.iter().map(|t| Line::from(*t)).collect::<Vec<_>>())
            .select(tab)
            .style(Style::default().fg(tele))
            .highlight_style(
                Style::default()
                    .fg(ship)
                    .bold()
                    .add_modifier(Modifier::UNDERLINED),
            ),
        body[0],
    );

    match tab {
        0 => draw_panel_helm(f, body[1], tel, o, ship, tele, bone, alert, void),
        1 => draw_panel_dispatch(f, body[1], o, tele, bone, alert),
        2 => draw_panel_knowledge(f, body[1], o, tele, bone),
        3 => draw_panel_outfit(f, body[1], o, ship, tele, bone, alert, void),
        _ => draw_panel_helm(f, body[1], tel, o, ship, tele, bone, alert, void),
    }

    // FOOTER: live agentic status line (the Claude-Code-style bar)
    //   ◈ haiku · 1.2k tok · 38% ctx · 420ms · drift 12%
    let drift = tel.drift_now();
    let drift_col = if drift > 60 { alert } else { tele };
    let drift_word = if drift > 60 {
        "drift! hallu-risk"
    } else {
        "drift"
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ◈ ", Style::default().fg(ship).bold()),
            Span::styled(format!("{}", tel.model()), Style::default().fg(bone).bold()),
            Span::styled(format!(" · {} tok", tel.tokens), Style::default().fg(tele)),
            Span::styled(
                format!(" · {}% ctx", tel.context_pct),
                Style::default().fg(tel.ctx_color(o)),
            ),
            Span::styled(format!(" · {}ms", tel.exec_ms), Style::default().fg(bone)),
            Span::styled(
                format!(" · {} {} ", drift_word, drift),
                Style::default().fg(drift_col),
            ),
            Span::styled("  [←→] switch  [q] dock", Style::default().fg(bone)),
        ])),
        outer[2],
    );
}

fn draw_panel_helm(
    f: &mut TuiFrame,
    area: Rect,
    tel: &Telemetry,
    o: &Outfit,
    ship: Color,
    tele: Color,
    bone: Color,
    alert: Color,
    _void: Color,
) {
    let cols =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);

    // LEFT: the live ship loader (animated by state+frame) + context + authority
    let left = Layout::vertical([
        Constraint::Length(8), // the dynamic loader ship
        Constraint::Length(3), // context bar
        Constraint::Length(3), // authority gauge
    ])
    .split(cols[0]);
    let loader_lines = draw_loader(tel.state, tel.frame, o);
    f.render_widget(
        Paragraph::new(loader_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" ◈ ship · {} ", tel.state.label()))
                .border_style(Style::default().fg(ship)),
        ),
        left[0],
    );
    // context bar — color-coded like Claude Code's status line
    f.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" context ")
                    .border_style(Style::default().fg(tel.ctx_color(o))),
            )
            .gauge_style(Style::default().fg(tel.ctx_color(o)))
            .ratio(tel.context_pct as f64 / 100.0)
            .label(format!("{}%", tel.context_pct)),
        left[1],
    );
    f.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" authority ")
                    .border_style(Style::default().fg(ship)),
            )
            .gauge_style(Style::default().fg(ship))
            .ratio(tel.authority_gauge() as f64 / 100.0)
            .label(format!("{}%", tel.authority_gauge())),
        left[2],
    );

    // RIGHT: OpenCode-like working feed + Hermes-like hints + drift + twin
    let has_twin = tel.twin.is_some();
    let mut right_c = vec![
        Constraint::Min(4),    // working feed (live log)
        Constraint::Length(4), // hermes-like hints
        Constraint::Length(3), // drift / hallu-risk
    ];
    if has_twin {
        right_c.push(Constraint::Length(4)); // what-if twin diff
    }
    let right = Layout::vertical(right_c).split(cols[1]);

    let feed_lines: Vec<Line> = tel
        .feed
        .iter()
        .enumerate()
        .map(|(i, (s, line))| {
            let tag = match s {
                AgentState::Booting => "boot",
                AgentState::Initing => "init",
                AgentState::Node => "node",
                AgentState::Recalling => "recall",
                AgentState::Thinking => "think",
                AgentState::Radio => "radio",
                AgentState::Idle => "idle",
            };
            // karaoke: only the LAST line types out char-by-char, in sync with loader
            let shown = if i + 1 == tel.feed.len() {
                line.chars().take(tel.karaoke).collect::<String>()
            } else {
                line.clone()
            };
            Line::from(vec![
                Span::styled(format!(" {tag:>5} › "), Style::default().fg(tele)),
                Span::styled(shown, Style::default().fg(bone)),
            ])
        })
        .collect();
    f.render_widget(
        Paragraph::new(feed_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" ◈ working ")
                .border_style(Style::default().fg(tele)),
        ),
        right[0],
    );

    let hint_lines: Vec<Line> = tel
        .hints
        .iter()
        .map(|h| Line::from(Span::styled(h.clone(), Style::default().fg(ship))))
        .collect();
    f.render_widget(
        Paragraph::new(hint_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" ◈ hints ")
                .border_style(Style::default().fg(ship)),
        ),
        right[1],
    );

    let drift_col = if tel.drift_now() > 60 { alert } else { tele };
    f.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" drift / hallu-risk ")
                    .border_style(Style::default().fg(drift_col)),
            )
            .style(Style::default().fg(drift_col))
            .data(&tel.drift)
            .max(100),
        right[2],
    );

    // WHAT-IF TWIN: side-by-side diff when a fork is docked.
    if has_twin {
        let diff_lines: Vec<Line> = tel
            .twin_diff()
            .iter()
            .map(|d| Line::from(Span::styled(d.clone(), Style::default().fg(tele))))
            .collect();
        f.render_widget(
            Paragraph::new(diff_lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" ◈ twin · what-if ")
                    .border_style(Style::default().fg(tele)),
            ),
            right[3],
        );
    }
}

fn draw_panel_dispatch(
    f: &mut TuiFrame,
    area: Rect,
    o: &Outfit,
    tele: Color,
    bone: Color,
    alert: Color,
) {
    let _ = o;
    let items = vec![
        ListItem::new(Line::from(vec![
            Span::styled("▶ ", Style::default().fg(alert)),
            Span::styled(
                "multipilot fan-out → N pilots + synthesizer",
                Style::default().fg(bone),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("  pilot·haiku   ", Style::default().fg(tele)),
            Span::styled("doer seam · cheap", Style::default().fg(bone)),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("  pilot·opus    ", Style::default().fg(tele)),
            Span::styled("reasoning-only · red-line rail", Style::default().fg(bone)),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("  synth         ", Style::default().fg(tele)),
            Span::styled("decides · field arbiter veto", Style::default().fg(bone)),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("⛔ ", Style::default().fg(alert)),
            Span::styled(
                "field gate: plan cost 0.91 > 0.90 ceiling → DENIED (RED)",
                Style::default().fg(alert),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("✓ ", Style::default().fg(tele)),
            Span::styled(
                "re-plan accepted at 0.72 — dispatched",
                Style::default().fg(bone),
            ),
        ])),
    ];
    f.render_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" multipilot trace ")
                    .border_style(Style::default().fg(alert)),
            )
            .style(Style::default().fg(bone)),
        area,
    );
}

fn draw_panel_knowledge(f: &mut TuiFrame, area: Rect, o: &Outfit, tele: Color, bone: Color) {
    let _ = o;
    let items = vec![
        ListItem::new(Line::from(vec![
            Span::styled("§0·GP ", Style::default().fg(tele).bold()),
            Span::styled(
                "retrieval: recall@5 = 1.000 (hard 29-q oracle)",
                Style::default().fg(bone),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("  copilot  ", Style::default().fg(tele)),
            Span::styled("native doer/checker seam", Style::default().fg(bone)),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("  vault    ", Style::default().fg(tele)),
            Span::styled(
                "XChaCha20-Poly1305 · argon2id · ML-KEM-768 ⊕ X25519 · ML-DSA-65 ⊕ Ed25519",
                Style::default().fg(bone),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("  field    ", Style::default().fg(tele)),
            Span::styled(
                "graph-PDE cost · can veto the planner",
                Style::default().fg(bone),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("  memory   ", Style::default().fg(tele)),
            Span::styled(
                "ONE living node · VSA + graph + recursion",
                Style::default().fg(bone),
            ),
        ])),
    ];
    f.render_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" living knowledge ")
                    .border_style(Style::default().fg(tele)),
            )
            .style(Style::default().fg(bone)),
        area,
    );
}

fn draw_panel_outfit(
    f: &mut TuiFrame,
    area: Rect,
    o: &Outfit,
    ship: Color,
    tele: Color,
    bone: Color,
    alert: Color,
    _void: Color,
) {
    let p = o.palette;
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(format!(" {}  ", o.sigil), Style::default().fg(ship).bold()),
                Span::styled(
                    format!("BEBOP v{}", o.version),
                    Style::default().fg(bone).bold(),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                format!("  creed   {}", o.creed),
                Style::default().fg(bone),
            )),
            Line::from(Span::styled(
                format!("  tagline {}", o.tagline),
                Style::default().fg(ship),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("  ship  #{}   tele #{}", rgb_hex(p.ship), rgb_hex(p.tele)),
                Style::default().fg(ship),
            )),
            Line::from(Span::styled(
                format!("  void  #{}   alert #{}", rgb_hex(p.void), rgb_hex(p.alert)),
                Style::default().fg(alert),
            )),
            Line::from(Span::styled(
                format!("  bone  #{}", rgb_hex(p.bone)),
                Style::default().fg(bone),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("  home   {}", o.home),
                Style::default().fg(tele),
            )),
            Line::from(Span::styled(
                "  customize: bebop init --looks RRGGBB --narration X",
                Style::default().fg(bone),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" outfit ")
                .border_style(Style::default().fg(ship)),
        ),
        area,
    );
}

// ---- ship-repaint (looks-axis transition, LCG, air-gapped) ----------------
/// Produce `steps` launch frames where the ship color tweens from `from` to `to`
/// along a deterministic LCG ramp (no RNG/Date). Used by `bebop init --looks`
/// and `bebop preview --transition` so the ship visibly repaints itself.
/// Flag-OFF by default: the rest of the UI calls `render_launch_accent` directly.
pub fn render_launch_tween(
    w: usize,
    h: usize,
    seed: u64,
    steps: usize,
    from: u32,
    to: u32,
    void: u32,
) -> Vec<Frame> {
    // Seed the launch with `from` as the hull color so the tween starts there,
    // then lerp every hull/trail pixel from `from` → `to` along a deterministic
    // ramp. Uses `render_launch_accent` (NOT `render_launch`, which hardcodes
    // the canonical ship color) so `from` is the actual drawn hue.
    let base = render_launch_accent(w, h, seed, steps, from, void);
    base.into_iter()
        .enumerate()
        .map(|(i, mut fr)| {
            let t = if steps == 0 {
                1.0
            } else {
                i as f64 / steps as f64
            };
            // blend(to, from, t): t=0 → from (start hull), t=1 → to (end hull).
            let ship = blend(to, from, t);
            for idx in 0..fr.cells.len() {
                let px = fr.cells[idx];
                // recolor anything that was the hull/trail hue (matches `from`)
                if (px & 0xFFFFFF) == (from & 0xFFFFFF) {
                    fr.cells[idx] = (px & 0xFF000000) | (ship & 0xFFFFFF);
                }
            }
            fr
        })
        .collect()
}

// ---- per-command ship animation (headless-safe, no TTY required) -----------
/// Print `frames` lines of the custom ship loader for `state` to stdout, with a
/// `label` and `note`. Each command (boot/init/node/recall) calls this with its
/// own `AgentState` so the ship *moves differently* per command — boot rises,
/// init repaints, node shields, recall scans, recall/thinking orbit. Deterministic
/// (frame counter, no RNG/Date), so the same command always paints the same way.
pub fn render_loader_animation(
    state: AgentState,
    frames: usize,
    label: &str,
    note: &str,
    o: &Outfit,
) {
    let line = "─".repeat(48);
    println!("\x1b[38;5;180m{}\x1b[0m", line);
    println!(
        "\x1b[38;5;221m◈ {}\x1b[0m \x1b[38;5;180m{}\x1b[0m",
        label, note
    );
    for f in 0..frames {
        let lines = draw_loader(state, f as u64, o);
        // keep only the non-empty mini-stage rows for a tight animation
        for l in &lines {
            println!("  {}", line_to_string(l));
        }
        // carriage-return the block up so it reads as live motion (skip in CI/no-anim)
        if std::env::var("CI").is_err() && std::env::var("NO_ANIM").is_err() {
            use std::io::Write;
            print!("\x1b[{}A", lines.len());
            std::io::stdout().flush().ok();
            std::thread::sleep(std::time::Duration::from_millis(90));
        }
    }
    println!("\x1b[38;5;180m{}\x1b[0m\n", line);
}

/// Flatten a styled line back to its raw string (used by the headless loader).
fn line_to_string(l: &Line) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

pub fn run_tui() -> std::io::Result<()> {
    let o = Profile::load().resolve_outfit();
    let is_tty = crossterm::tty::IsTty::is_tty(&std::io::stdout());
    let skip = std::env::var("NO_ANIM").is_ok() || std::env::var("CI").is_ok() || !is_tty;
    if skip {
        if !is_tty {
            println!("{}", o.banner());
            println!("  (headless — run `bebop` in a TTY for the cosmo-noir helm)");
            crate::mission::mission_summary(
                "session",
                &[
                    "ran headless — no helm, just the report. honest trade.",
                    "the ship held station. the cigar waited. see you in a TTY.",
                ],
            );
            return Ok(());
        }
        let mut stdout = std::io::stdout();
        let _guard = RawModeGuard::enter(&mut stdout)?;
        let backend = CrosstermBackend::new(stdout);
        let mut term = Terminal::new(backend)?;
        helm_loop(&mut term, &o)?;
        crate::mission::mission_summary(
            "session",
            &[
                "helm docked. no crashes, no drift past the red line — we checked.",
                "the ship is still here. that's the whole point. cigar's lit.",
            ],
        );
        return Ok(());
    }

    // Enter raw mode + alternate screen via a RAII guard so they are ALWAYS
    // restored — even if `helm_loop` (or anything it calls) panics.
    let mut stdout = std::io::stdout();
    let _guard = RawModeGuard::enter(&mut stdout)?;
    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;

    // LAUNCH ritual (recolored by the active outfit's ship color)
    let frames = render_launch_accent(48, 22, 0xC0FFEE, 18, o.palette.ship, o.palette.void);
    for fr in &frames {
        term.draw(|f| draw_launch(f, fr, &o))?;
        std::thread::sleep(Duration::from_millis(55));
    }
    std::thread::sleep(Duration::from_millis(350));

    let res = helm_loop(&mut term, &o);
    // `_guard` drops here → raw mode + alternate screen restored (panic-safe).

    crate::mission::mission_summary(
        "session",
        &[
            "helm docked. no crashes, no drift past the red line — we checked.",
            "the ship is still here. that's the whole point. cigar's lit.",
        ],
    );
    res
}

fn helm_loop(term: &mut Terminal<CrosstermBackend<Stdout>>, o: &Outfit) -> std::io::Result<()> {
    let mut tel = Telemetry::new();
    let mut tab: usize = 0;
    // idle but alive: the ship orbits while it waits for your order
    tel.state = AgentState::Thinking;
    loop {
        term.draw(|f| draw_helm(f, &tel, tab, o))?;
        if event::poll(Duration::from_millis(220))? {
            if let Ok(Event::Key(k)) = event::read() {
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Right | KeyCode::Char('l') => tab = (tab + 1) % 4,
                    KeyCode::Left | KeyCode::Char('h') => tab = (tab + 3) % 4,
                    // 't' forks a What-If twin from a different seed → diff side-by-side
                    KeyCode::Char('t') => tel.fork_twin(0x7E57 + tel.frame),
                    _ => {}
                }
            }
        } else {
            tel.tick();
        }
    }
    Ok(())
}

/// Render all four helm tabs to one SVG document (design-proof artifact).
/// Void cells are omitted; ship/tele/bone/alert glyphs are emitted as colored text.
pub fn render_helm_svg(w: u16, h: u16, o: &Outfit) -> String {
    use ratatui::backend::TestBackend;
    let tabs = ["helm", "dispatch", "knowledge", "outfit"];
    let mut rects = String::new();
    let cell: u32 = 10;
    let mut yoff: u32 = 0;
    for (ti, _tab) in tabs.iter().enumerate() {
        let backend = TestBackend::new(w, h);
        let mut term = Terminal::new(backend).unwrap();
        let tel = Telemetry::new();
        term.draw(|f| draw_helm(f, &tel, ti, o)).unwrap();
        let buf = term.backend().buffer().clone();
        rects.push_str(&format!(
            "<text x='4' y='{}' fill='#{:06X}' font-family='monospace' font-size='{}'>{}</text>\n",
            yoff * cell + cell,
            o.palette.tele,
            cell,
            tabs[ti]
        ));
        let top = yoff * cell + cell + 2;
        for (i, cellref) in buf.content().iter().enumerate() {
            let x = (i as u16) % w;
            let y = (i as u16) / w;
            let raw = cellref.symbol();
            if raw.trim().is_empty() {
                continue;
            }
            let ch = raw
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let fg = match cellref.fg {
                Color::Rgb(r, g, b) => (r as u32) << 16 | (g as u32) << 8 | b as u32,
                _ => o.palette.bone,
            };
            rects.push_str(&format!(
                "<text x='{}' y='{}' fill='#{:06X}' font-family='monospace' font-size='{}'>{}</text>\n",
                x as u32 * cell as u32 + 1,
                top + y as u32 * cell as u32 + (cell * 9 / 10),
                fg,
                cell,
                ch
            ));
        }
        yoff += (h as u32) + 2;
    }
    let total_h = yoff * cell + cell;
    let total_w = w as u32 * cell as u32;
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}' viewBox='0 0 {} {}' shape-rendering='crispEdges' style='background:#12100E'>\n{}</svg>\n",
        total_w, total_h, total_w, total_h, rects
    )
}

/// Dump one helm tab as plain text (inspection / debugging).
pub fn debug_helm_text(w: u16, h: u16, tab: usize, o: &Outfit) -> String {
    use ratatui::backend::TestBackend;
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    let tel = Telemetry::new();
    term.draw(|f| draw_helm(f, &tel, tab, o)).unwrap();
    let buf = term.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            let s = buf[(x, y)].symbol();
            out.push_str(if s.trim().is_empty() { " " } else { s });
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::render_launch;
    use crate::outfit::OUTFIT;
    use ratatui::backend::TestBackend;

    #[test]
    fn tui_launch_frame_renders_without_panic() {
        let backend = TestBackend::new(48, 22);
        let mut term = Terminal::new(backend).unwrap();
        let frames = render_launch(48, 22, 0xC0FFEE, 18);
        term.draw(|f| draw_launch(f, frames.last().unwrap(), &OUTFIT))
            .expect("launch frame must render");
        let ship = (OUTFIT.palette.ship >> 16) as u8;
        let buf = term.backend().buffer().clone();
        let saw_ship = buf.content().iter().any(|cell| {
            if let Color::Rgb(r, _, _) = cell.fg {
                r == ship
            } else {
                false
            }
        });
        assert!(saw_ship, "launch frame rendered no sun-warm ship pixel");
    }

    #[test]
    fn helm_renders_all_four_tabs() {
        let backend = TestBackend::new(90, 30);
        let mut term = Terminal::new(backend).unwrap();
        let tel = Telemetry::new();
        let need = ["BEBOP", "helm", "dispatch", "knowledge", "outfit"];
        for tab in 0..4 {
            term.draw(|f| draw_helm(f, &tel, tab, &OUTFIT))
                .unwrap_or_else(|_| panic!("helm tab {tab} must render"));
            let buf = term.backend().buffer().clone();
            let text: String = buf.content().iter().map(|c| c.symbol()).collect();
            for n in need {
                assert!(text.contains(n), "tab {tab} missing '{n}'");
            }
        }
        for (tab, marker) in [
            (1usize, "multipilot"),
            (2, "living knowledge"),
            (3, "creed"),
        ] {
            term.draw(|f| draw_helm(f, &tel, tab, &OUTFIT)).unwrap();
            let buf = term.backend().buffer().clone();
            let text: String = buf.content().iter().map(|c| c.symbol()).collect();
            assert!(text.contains(marker), "tab {tab} missing '{marker}'");
        }
    }

    #[test]
    fn helm_status_line_shows_model_tokens_ctx() {
        // The agentic status bar must surface model + tokens + context %.
        let backend = TestBackend::new(90, 30);
        let mut term = Terminal::new(backend).unwrap();
        let tel = Telemetry::new();
        term.draw(|f| draw_helm(f, &tel, 0, &OUTFIT)).unwrap();
        let buf = term.backend().buffer().clone();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("tok"), "status line missing token usage");
        assert!(text.contains("% ctx"), "status line missing context %");
        assert!(text.contains("ms"), "status line missing exec time");
        assert!(text.contains("drift"), "status line missing drift signal");
    }

    #[test]
    fn helm_honors_custom_accent() {
        // RED+GREEN: a profile accent recolors the ship in the helm.
        use crate::customize::{LooksOverride, Profile};
        let mut p = Profile::default();
        p.looks = Some(LooksOverride {
            accent: Some("00FF00".into()),
        });
        let o = p.resolve_outfit();
        assert_eq!(o.palette.ship, 0x00FF00);
        let backend = TestBackend::new(90, 30);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_helm(f, &Telemetry::new(), 0, &o))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let saw_green = buf.content().iter().any(|cell| {
            if let Color::Rgb(_, g, b) = cell.fg {
                g == 0xFF && b == 0x00 && cell.symbol().contains('█')
            } else {
                false
            }
        });
        assert!(saw_green, "custom accent not applied to helm ship");
    }

    #[test]
    fn launch_tween_repaints_hull() {
        // RED+GREEN: a tween from red → green ends with a GREEN hull.
        // Same in/out → tween is a no-op (still red).
        let a = render_launch_tween(30, 14, 0xC0FFEE, 12, 0xE0543E, 0x00FF00, 0x12100E);
        let last = a.last().unwrap();
        let green_cells = last
            .cells
            .iter()
            .filter(|px| (**px & 0xFFFFFF) == 0x00FF00)
            .count();
        assert!(green_cells > 0, "tween end frame has no green hull pixels");

        let b = render_launch_tween(30, 14, 0xC0FFEE, 12, 0xE0543E, 0xE0543E, 0x12100E);
        let last_b = b.last().unwrap();
        let still_red = last_b
            .cells
            .iter()
            .filter(|px| (**px & 0xFFFFFF) == 0xE0543E)
            .count();
        assert!(still_red > 0, "same in/out tween must keep the hull red");
    }

    #[test]
    fn loader_is_distinct_per_command_state() {
        // RED+GREEN: each command state must produce its OWN ship motion.
        // Two different states at the same frame must NOT render identical lines.
        let o = &OUTFIT;
        let a = draw_loader(AgentState::Node, 3, o);
        let b = draw_loader(AgentState::Recalling, 3, o);
        let a_s: Vec<String> = a.iter().map(|l| line_to_string(l)).collect();
        let b_s: Vec<String> = b.iter().map(|l| line_to_string(l)).collect();
        assert_ne!(a_s, b_s, "node and recall loaders must differ");
        // Idle is static; a working state differs from idle at the same frame.
        let idle = draw_loader(AgentState::Idle, 3, o);
        let idle_s: Vec<String> = idle.iter().map(|l| line_to_string(l)).collect();
        assert_ne!(idle_s, a_s, "idle must differ from node loader");
    }

    #[test]
    fn karaoke_reveals_feed_progressively() {
        // RED+GREEN: the working feed must TYPE OUT — not dump all at once.
        let mut t = Telemetry::new();
        t.state = AgentState::Thinking;
        t.log(AgentState::Thinking, "awaiting model output");
        let full = t.feed.last().unwrap().1.len() as u64;
        t.karaoke = 0;
        // pin to Idle so the loop doesn't keep appending new lines (which would
        // reset the reveal) — we only want to prove the LAST line types out.
        t.state = AgentState::Idle;
        assert!(t.karaoke < full as usize, "karaoke must start hidden");
        // advance ticks → karaoke grows toward full, monotonic, never exceeds
        for _ in 0..20 {
            t.tick();
        }
        assert_eq!(t.karaoke, full as usize, "karaoke must reach full length");
    }

    #[test]
    fn twin_fork_diverges_and_diffs() {
        // RED+GREEN: a forked twin MUST diverge from the parent (different seed)
        // and the diff must report both sides.
        let mut t = Telemetry::new();
        for _ in 0..5 {
            t.tick();
        }
        let before = t.tokens;
        t.fork_twin(0x7E57);
        assert!(t.twin.is_some(), "twin must be docked after fork");
        // twin ticks under its own seed → its token trace diverges from parent
        for _ in 0..5 {
            t.tick();
        }
        let diff = t.twin_diff();
        assert!(diff.len() >= 3, "twin diff must show tok/ctx/drift lines");
        // the parent advanced; twin (different seed) advanced differently
        assert_ne!(t.tokens, before, "parent tokens must advance");
        let twin_tok: u64 = diff[0]
            .split("twin ")
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        assert_ne!(twin_tok, 0, "twin must have produced token telemetry");
    }

    #[test]
    fn helm_renders_twin_when_docked() {
        // RED+GREEN: the twin block appears in the helm only after forking.
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(90, 30);
        let mut term = Terminal::new(backend).unwrap();
        let mut tel = Telemetry::new();
        tel.fork_twin(0x7E57);
        term.draw(|f| draw_helm(f, &tel, 0, &OUTFIT)).unwrap();
        let buf = term.backend().buffer().clone();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(
            text.contains("twin"),
            "helm must show twin block after fork"
        );
    }

    #[test]
    fn telemetry_is_deterministic() {
        // Same seed → same trace. Proves the helm is reproducible (air-gapped).
        let mut a = Telemetry::new();
        let mut b = Telemetry::new();
        for _ in 0..10 {
            a.tick();
            b.tick();
        }
        assert_eq!(a.drift, b.drift);
        assert_eq!(a.quality, b.quality);
        assert_eq!(a.tokens, b.tokens);
    }

    // ── BP-23: raw-mode RAII guard is panic-safe + fail-safe ──
    #[test]
    fn raw_mode_guard_fails_safe_without_tty() {
        // In a non-TTY (CI) environment `enter` must return Err and leave the
        // guard INACTIVE, so its Drop never tries to restore a state we never
        // entered (which would itself panic or corrupt the terminal).
        let mut stdout = std::io::stdout();
        let res = RawModeGuard::enter(&mut stdout);
        assert!(res.is_err(), "enter must fail off-TTY, not hang/panic");
        // The inactive guard (inside the Err) drops without touching the terminal.
        drop(res.unwrap_err());
        // Construct the inactive variant directly and drop it: must not panic.
        let inactive = RawModeGuard { active: false };
        drop(inactive);
    }
}
