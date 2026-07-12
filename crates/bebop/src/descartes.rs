//! descartes.rs — DESCARTES-SQUARE auto-comparison (category N3).
//!
//! For proposed changes / research / analysis / library loading, auto-emit a
//! 2x2 comparison: exact ADVANTAGES / exact DISADVANTAGES of option A vs B
//! (Cartesian-square logic). Pure data + render; no new deps.

/// One side of a comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Square {
    pub option: String,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
}

impl Square {
    pub fn new(option: &str, pros: &[&str], cons: &[&str]) -> Self {
        Square {
            option: option.to_string(),
            pros: pros.iter().map(|s| s.to_string()).collect(),
            cons: cons.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Compare two options, returning both squares (the "2x2": each option's
/// pros/cons are the four quadrants).
pub fn compare(a: Square, b: Square) -> (Square, Square) {
    (a, b)
}

/// Render as a compact CLI table (Markdown-ish, pipe-safe).
pub fn render(a: &Square, b: &Square) -> String {
    let mut s = String::new();
    s.push_str(&format!("│ {:<22} │ {:<22} │\n", a.option, b.option));
    s.push_str("├──────────────────────┼──────────────────────┤\n");
    let rows = a.pros.len().max(a.cons.len()).max(b.pros.len()).max(b.cons.len());
    for i in 0..rows {
        let ap = a.pros.get(i).map(|s| s.as_str()).unwrap_or("");
        let bp = b.pros.get(i).map(|s| s.as_str()).unwrap_or("");
        let ac = a.cons.get(i).map(|s| s.as_str()).unwrap_or("");
        let bc = b.cons.get(i).map(|s| s.as_str()).unwrap_or("");
        if i < a.pros.len() || i < b.pros.len() {
            s.push_str(&format!("│ + {:<20} │ + {:<20} │\n", ap, bp));
        }
        if i < a.cons.len() || i < b.cons.len() {
            s.push_str(&format!("│ - {:<20} │ - {:<20} │\n", ac, bc));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_returns_both_sides() {
        let a = Square::new("rust", &["fast", "safe"], &["verbose"]);
        let b = Square::new("python", &["quick to write"], &["slow", "gil"]);
        let (x, y) = compare(a.clone(), b.clone());
        assert_eq!(x, a);
        assert_eq!(y, b);
        assert!(x.pros.contains(&"fast".to_string()));
        assert!(y.cons.contains(&"gil".to_string()));
    }

    #[test]
    fn render_has_both_options_and_pros_cons() {
        let a = Square::new("zenoh", &["mesh-native"], &["new dep"]);
        let b = Square::new("http", &["battle-tested"], &["no pubsub"]);
        let r = render(&a, &b);
        assert!(r.contains("zenoh"));
        assert!(r.contains("http"));
        assert!(r.contains("+ mesh-native"));
        assert!(r.contains("- new dep"));
    }
}
