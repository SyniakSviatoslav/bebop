//! panels.rs — TUI PANEL CONTENT (categories D scoreboard / E minimap / H drift).
//!
//! Pure content builders (return `String` / data, no ratatui lifetime headaches).
//! `tui.rs` wraps the returned `String` in a `Paragraph` at render time. This
//! keeps the panel logic testable without a terminal frame.
//!
//! ponytail: minimap reads node positions from callers (the connection graph is
//! advisory); no wavefield dependency here so the panel stays a pure function.

use crate::telemetry::Telemetry;

/// D — SCOREBOARD: returns the memory-pressure ratio (0..1) + a KPI text line.
pub fn scoreboard(tel: &Telemetry, lanes_busy: usize, lanes_max: usize) -> (f32, String) {
    let pressure = tel.mem_pressure();
    let kpi = format!(
        "load {:.2} · mem {}/{} MB · lanes {}/{}",
        tel.load_1m,
        tel.mem_avail_kb / 1024,
        tel.mem_total_kb / 1024,
        lanes_busy,
        lanes_max
    );
    (pressure, kpi)
}

/// E — MINIMAP: a small ASCII grid of the connection graph (advisory).
pub fn minimap(nodes: &[(f32, f32)], cols: u16, rows: u16) -> String {
    let mut grid: Vec<Vec<char>> = vec![vec!['·'; cols as usize]; rows as usize];
    for &(x, y) in nodes {
        let cx =
            ((x.clamp(0.0, 1.0) * (cols as f32 - 1.0)).round() as usize).min(cols as usize - 1);
        let cy =
            ((y.clamp(0.0, 1.0) * (rows as f32 - 1.0)).round() as usize).min(rows as usize - 1);
        grid[cy][cx] = '◆';
    }
    grid.iter()
        .map(|r| r.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// H — DRIFT PANEL: render systems-thinking / architecture drift hits as text.
pub fn drift_panel(hits: &[crate::drift::Drift]) -> String {
    if hits.is_empty() {
        return "✓ no systems-thinking / architecture drift detected".to_string();
    }
    hits.iter()
        .map(|d| format!("⚠ {:?}: {}", d.practice, d.detail))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drift::{Drift, DriftPolicy, Practice};

    #[test]
    fn scoreboard_pressure_bounded() {
        // GREEN: pressure always 0..=1; KPI line is non-empty.
        let mut t = Telemetry::default();
        t.mem_total_kb = 1000;
        t.mem_avail_kb = 200;
        let (p, kpi) = scoreboard(&t, 2, 4);
        assert!((p - 0.8).abs() < 1e-3, "pressure should be 0.8, got {p}");
        assert!(kpi.contains("lanes 2/4"));
    }

    #[test]
    fn scoreboard_zero_when_unknown() {
        // GREEN: missing telemetry → 0 pressure (never negative / never panic).
        let (p, _kpi) = scoreboard(&Telemetry::default(), 0, 4);
        assert_eq!(p, 0.0);
    }

    #[test]
    fn minimap_places_node_in_grid() {
        // GREEN: a node at (0.5,0.5) lands inside the grid, not out of bounds.
        let s = minimap(&[(0.5, 0.5)], 10, 5);
        assert!(s.contains('◆'), "node glyph missing from minimap");
        assert_eq!(s.lines().count(), 5, "minimap should have 5 rows");
    }

    #[test]
    fn minimap_empty_grid_when_no_nodes() {
        // GREEN: no nodes → only placeholder dots, no glyph.
        let s = minimap(&[], 4, 2);
        assert!(!s.contains('◆'));
    }

    #[test]
    fn drift_panel_shows_clean_when_empty() {
        // GREEN: no drift → honest "no drift" (no false alarm).
        assert!(drift_panel(&[]).contains("no systems-thinking"));
    }

    #[test]
    fn drift_panel_lists_hits() {
        // GREEN: a detected drift surfaces in the panel text.
        let hits = vec![Drift {
            practice: Practice::NewGlobalDep,
            detail: "introduces a new global dependency".into(),
        }];
        let s = drift_panel(&hits);
        assert!(s.contains("NewGlobalDep"), "drift hit not shown");
    }

    #[test]
    fn drift_policy_default_watches_global_dep() {
        // GREEN: default policy must watch NewGlobalDep (the systems-thinking gate).
        let p = DriftPolicy::default();
        assert!(
            p.watch.contains(&Practice::NewGlobalDep),
            "default drift policy must watch new-global-dep"
        );
    }
}
