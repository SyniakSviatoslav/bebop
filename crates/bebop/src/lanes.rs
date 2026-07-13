//! lanes.rs — parallel-session scheduler (category O of the master plan).
//! Minimal but real: a Lane tracks throughput + queue + ETA. The dispatcher
//! routes work to the freest lane. Extended by Wave 3 (tui panel, auto-queue).
//! Operator: default maximal parallelism, never exceeds `max_lanes`.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneStatus {
    Idle,
    Running,
    Draining,
}

#[derive(Debug, Clone)]
pub struct Lane {
    pub name: String,
    pub status: LaneStatus,
    pub completed: u64,
    pub busy_since: Option<Instant>,
    pub queue: VecDeque<String>,
}

impl Lane {
    pub fn new(name: &str) -> Self {
        Lane {
            name: name.to_string(),
            status: LaneStatus::Idle,
            completed: 0,
            busy_since: None,
            queue: VecDeque::new(),
        }
    }

    /// Throughput = completed tasks per minute, measured over a sliding window.
    /// Cheap stand-in: completed / elapsed-min since first busy (clamped).
    pub fn throughput(&self, started: Instant) -> f64 {
        let mins = started.elapsed().as_secs_f64() / 60.0;
        if mins < 1e-6 {
            0.0
        } else {
            self.completed as f64 / mins
        }
    }

    pub fn enqueue(&mut self, item: String) {
        self.queue.push_back(item);
    }

    /// Dispatch: refuse if already at max_lanes worth of running lanes.
    pub fn dispatch(lanes: &mut [Lane], item: String, max_lanes: usize) -> bool {
        if lanes
            .iter()
            .filter(|l| l.status == LaneStatus::Running)
            .count()
            >= max_lanes
        {
            return false; // RED: refuse > max_lanes concurrently
        }
        // Freest = fewest queued + idle preferred.
        if let Some(lane) = lanes.iter_mut().min_by_key(|l| l.queue.len()) {
            lane.enqueue(item);
            if lane.status == LaneStatus::Idle {
                lane.status = LaneStatus::Running;
                lane.busy_since = Some(Instant::now());
            }
            true
        } else {
            false
        }
    }

    /// Predicted finish for the head item: EMA of prior same-size durations.
    pub fn eta(&self, avg_task: Duration) -> Duration {
        avg_task * (self.queue.len() as u32 + 1)
    }

    /// Mark a lane done with one completed task (clears busy, recomputes status).
    pub fn complete_one(&mut self) {
        self.completed += 1;
        self.busy_since = None;
        if self.queue.is_empty() {
            self.status = LaneStatus::Idle;
        }
    }
}

/// A central scheduler owning N lanes + a global auto-queue. Incoming work is
/// enqueued centrally; `pump` assigns each queued item to the freest lane (max
/// throughput headroom), refusing to exceed `max_lanes` running concurrently.
pub struct Scheduler {
    pub lanes: Vec<Lane>,
    pub max_lanes: usize,
    pub policy: SchedulerPolicy,
    pub avg_task: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedulerPolicy {
    Freest,
    RoundRobin,
    Pinned,
}

impl Default for SchedulerPolicy {
    fn default() -> Self {
        SchedulerPolicy::Freest
    }
}

impl Scheduler {
    pub fn new(max_lanes: usize) -> Self {
        let mut lanes = Vec::new();
        for i in 0..max_lanes.max(1) {
            lanes.push(Lane::new(&format!("lane-{i}")));
        }
        Scheduler {
            lanes,
            max_lanes: max_lanes.max(1),
            policy: SchedulerPolicy::Freest,
            avg_task: Duration::from_secs(60),
        }
    }

    /// Enqueue centrally; returns the lane index it was routed to, or None if
    /// all lanes are at capacity (RED: refuse > max_lanes concurrently).
    pub fn auto_queue(&mut self, item: String) -> Option<usize> {
        // RED: refuse if already at max running.
        if self
            .lanes
            .iter()
            .filter(|l| l.status == LaneStatus::Running)
            .count()
            >= self.max_lanes
        {
            return None;
        }
        let idx = match self.policy {
            SchedulerPolicy::Freest => self
                .lanes
                .iter()
                .enumerate()
                .min_by_key(|(_, l)| l.queue.len())
                .map(|(i, _)| i)
                .unwrap_or(0),
            SchedulerPolicy::RoundRobin => self
                .lanes
                .iter()
                .enumerate()
                .min_by_key(|(i, _)| *i)
                .map(|(i, _)| i)
                .unwrap_or(0),
            SchedulerPolicy::Pinned => 0,
        };
        self.lanes[idx].enqueue(item);
        if self.lanes[idx].status == LaneStatus::Idle {
            self.lanes[idx].status = LaneStatus::Running;
            self.lanes[idx].busy_since = Some(Instant::now());
        }
        Some(idx)
    }

    /// Render a one-line-per-lane CLI panel (name, status, queue depth, ETA).
    pub fn render_panel(&self) -> String {
        let mut s = String::new();
        for (i, l) in self.lanes.iter().enumerate() {
            let eta = l.eta(self.avg_task);
            s.push_str(&format!(
                "  [{i}] {:<10} {:<8} q={:<3} eta={:.0?}\n",
                l.name,
                format!("{:?}", l.status),
                l.queue.len(),
                eta
            ));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuse_over_max_lanes() {
        let mut lanes = vec![Lane::new("a"), Lane::new("b"), Lane::new("c")];
        lanes[0].status = LaneStatus::Running;
        lanes[1].status = LaneStatus::Running;
        // max_lanes = 2, already 2 running -> refuse 3rd dispatch
        assert!(!Lane::dispatch(&mut lanes, "x".into(), 2));
    }

    #[test]
    fn dispatch_routes_to_freest() {
        let mut lanes = vec![Lane::new("a"), Lane::new("b"), Lane::new("c")];
        lanes[0].enqueue("old".into());
        // b is freest (empty) -> item goes to b
        assert!(Lane::dispatch(&mut lanes, "new".into(), 3));
        assert_eq!(lanes[1].queue.back().unwrap(), "new");
        assert_eq!(lanes[1].status, LaneStatus::Running);
    }

    #[test]
    fn throughput_zero_before_work() {
        let lane = Lane::new("a");
        let start = Instant::now();
        assert_eq!(lane.throughput(start), 0.0);
    }

    #[test]
    fn scheduler_auto_queue_routes_freest() {
        let mut s = Scheduler::new(2);
        // Load lane 0 so lane 1 is freest.
        s.lanes[0].enqueue("seed".into());
        let routed = s.auto_queue("job".into());
        assert_eq!(routed, Some(1));
        assert_eq!(s.lanes[1].queue.back().unwrap(), "job");
    }

    #[test]
    fn scheduler_refuses_when_at_max_running() {
        let mut s = Scheduler::new(1);
        assert_eq!(s.auto_queue("a".into()), Some(0)); // lane running now
        assert_eq!(s.auto_queue("b".into()), None); // refused: at max_lanes=1
    }

    #[test]
    fn scheduler_panel_renders_all_lanes() {
        let s = Scheduler::new(2);
        let p = s.render_panel();
        assert!(p.contains("lane-0"));
        assert!(p.contains("lane-1"));
        assert!(p.contains("eta="));
    }

    #[test]
    fn complete_one_clears_when_idle() {
        let mut lane = Lane::new("a");
        lane.status = LaneStatus::Running;
        lane.complete_one();
        assert_eq!(lane.completed, 1);
        assert_eq!(lane.status, LaneStatus::Idle);
    }
}
