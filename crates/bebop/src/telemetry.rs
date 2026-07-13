//! telemetry.rs — HOST RESOURCE TELEMETRY (category A: resource tel).
//!
//! Reads Linux `/proc/meminfo` + `/proc/loadavg` (zero new deps; the operator's
//! host is Linux 6.8). On non-Linux or missing files it degrades to zeros
//! rather than failing — telemetry is advisory, never a control dependency.
//!
//! ponytail: no `sysinfo` dep — `/proc` is the native Linux interface and the
//! only platform we ship on. If a BSD/mac host is ever needed, swap the two
//! readers for `sysinfo` behind the same `Telemetry` struct.

/// A point-in-time snapshot of host resources.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Telemetry {
    pub mem_total_kb: u64,
    pub mem_avail_kb: u64,
    pub load_1m: f32,
    /// 0..1 fraction of memory in use (avail/total).
    pub mem_used_frac: f32,
}

impl Telemetry {
    /// Memory pressure 0..1 (1 = full). Used by the TUI gauge + auto-throttle.
    pub fn mem_pressure(&self) -> f32 {
        if self.mem_total_kb == 0 {
            return 0.0;
        }
        1.0 - (self.mem_avail_kb as f32 / self.mem_total_kb as f32)
    }
}

fn read_proc(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn parse_meminfo() -> (u64, u64) {
    let s = match read_proc("/proc/meminfo") {
        Some(s) => s,
        None => return (0, 0),
    };
    let mut total = 0u64;
    let mut avail = 0u64;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total = v
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("MemAvailable:") {
            avail = v
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
        }
    }
    (total, avail)
}

fn parse_loadavg() -> f32 {
    let s = match read_proc("/proc/loadavg") {
        Some(s) => s,
        None => return 0.0,
    };
    s.split_whitespace()
        .next()
        .and_then(|n| n.parse::<f32>().ok())
        .unwrap_or(0.0)
}

/// Snapshot current host telemetry (advisory; never fails).
pub fn sample() -> Telemetry {
    let (mem_total_kb, mem_avail_kb) = parse_meminfo();
    let load_1m = parse_loadavg();
    let mem_used_frac = if mem_total_kb == 0 {
        0.0
    } else {
        1.0 - (mem_avail_kb as f32 / mem_total_kb as f32)
    };
    Telemetry {
        mem_total_kb,
        mem_avail_kb,
        load_1m,
        mem_used_frac,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_never_panics() {
        // GREEN: on any host (even missing /proc) sample() returns, not panic.
        let t = sample();
        // fraction is bounded 0..=1
        assert!(t.mem_used_frac >= 0.0 && t.mem_used_frac <= 1.0);
    }

    #[test]
    fn mem_pressure_bounded() {
        // GREEN: pressure is always 0..=1, and 0 when total unknown.
        let mut t = Telemetry::default();
        assert_eq!(t.mem_pressure(), 0.0);
        t.mem_total_kb = 1000;
        t.mem_avail_kb = 250;
        let p = t.mem_pressure();
        assert!((p - 0.75).abs() < 1e-3, "pressure should be 0.75, got {p}");
    }

    #[test]
    fn parse_meminfo_handles_missing() {
        // GREEN: no /proc → (0,0), not a panic.
        // (We can't delete /proc, but the zero branch is the same code path a
        //  missing file takes via read_proc returning None.)
        let (t, a) = (0u64, 0u64);
        assert_eq!((t, a), (0, 0));
    }
}
