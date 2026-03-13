//! Process-level daemon statistics — atomic counters + lightweight histograms.
//!
//! Phase 4 PR1: Zero external deps, exposed via MCP health endpoint.
//! Histogram uses a fixed ring buffer (1000 samples) for P50/P95/P99.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Fixed-size ring buffer histogram for percentile calculations.
/// Stores last N samples; percentiles computed on-demand (lazy).
pub(crate) struct Histogram {
    samples: Mutex<Vec<u64>>,
    capacity: usize,
}

impl Histogram {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: Mutex::new(Vec::with_capacity(capacity)),
            capacity,
        }
    }

    /// Record a sample (microseconds). O(1) amortized.
    pub fn record(&self, value_us: u64) {
        if let Ok(mut s) = self.samples.lock() {
            if s.len() >= self.capacity {
                s.remove(0); // evict oldest
            }
            s.push(value_us);
        }
    }

    /// Compute percentiles. Returns (p50, p95, p99) in microseconds.
    /// Returns (0, 0, 0) if no samples.
    pub fn percentiles(&self) -> (u64, u64, u64) {
        let sorted = match self.samples.lock() {
            Ok(s) => {
                let mut v = s.clone();
                v.sort_unstable();
                v
            }
            Err(_) => return (0, 0, 0),
        };
        if sorted.is_empty() {
            return (0, 0, 0);
        }
        let len = sorted.len();
        let p50 = sorted[(len as f64 * 0.50) as usize];
        let p95 = sorted[((len as f64 * 0.95) as usize).min(len - 1)];
        let p99 = sorted[((len as f64 * 0.99) as usize).min(len - 1)];
        (p50, p95, p99)
    }

}

/// Process-level daemon statistics. All counters are monotonically increasing.
pub(crate) struct DaemonStats {
    // EventBus
    pub events_published: AtomicU64,
    pub events_consumed_extraction: AtomicU64,
    pub events_consumed_submit: AtomicU64,
    pub events_consumed_decision: AtomicU64,
    pub events_lagged_extraction: AtomicU64,
    pub events_lagged_submit: AtomicU64,
    pub events_lagged_decision: AtomicU64,
    pub events_lagged_total_skipped: AtomicU64,

    // DB Executor
    pub db_exec_runs: AtomicU64,
    pub db_exec_total_us: AtomicU64,
    pub db_exec_latency: Histogram,

    // Gemini Client
    pub gemini_requests: AtomicU64,
    pub gemini_errors: AtomicU64,
    pub gemini_retries: AtomicU64,
    pub gemini_latency: Histogram,

    // Autopilot
    pub autopilot_ticks: AtomicU64,
    pub autopilot_total_ms: AtomicU64,
    pub autopilot_latency: Histogram,

    // Task dispatch
    pub tasks_dispatched: AtomicU64,
    pub tasks_completed: AtomicU64,

    // Decision Engine
    pub decisions_tier1_hit: AtomicU64,
    pub decisions_tier2_hit: AtomicU64,
    pub decisions_tier3_dispatched: AtomicU64,

    // Context Prefetch v2
    pub prefetch_total: AtomicU64,
    pub prefetch_intent_chat: AtomicU64,
    pub prefetch_intent_code: AtomicU64,
    pub prefetch_intent_general: AtomicU64,
    pub prefetch_bypass: AtomicU64,
    pub prefetch_router_calls: AtomicU64,
    pub prefetch_router_errors: AtomicU64,
    pub prefetch_fallback: AtomicU64,
    pub prefetch_router_latency: Histogram,
    pub prefetch_cosine_filtered: AtomicU64,
    pub prefetch_devops_heuristic: AtomicU64,
    pub prefetch_skill_trigger_hit: AtomicU64,
}

impl DaemonStats {
    pub fn new() -> Self {
        Self {
            events_published: AtomicU64::new(0),
            events_consumed_extraction: AtomicU64::new(0),
            events_consumed_submit: AtomicU64::new(0),
            events_consumed_decision: AtomicU64::new(0),
            events_lagged_extraction: AtomicU64::new(0),
            events_lagged_submit: AtomicU64::new(0),
            events_lagged_decision: AtomicU64::new(0),
            events_lagged_total_skipped: AtomicU64::new(0),
            db_exec_runs: AtomicU64::new(0),
            db_exec_total_us: AtomicU64::new(0),
            db_exec_latency: Histogram::new(1000),
            gemini_requests: AtomicU64::new(0),
            gemini_errors: AtomicU64::new(0),
            gemini_retries: AtomicU64::new(0),
            gemini_latency: Histogram::new(1000),
            autopilot_ticks: AtomicU64::new(0),
            autopilot_total_ms: AtomicU64::new(0),
            autopilot_latency: Histogram::new(1000),
            tasks_dispatched: AtomicU64::new(0),
            tasks_completed: AtomicU64::new(0),
            decisions_tier1_hit: AtomicU64::new(0),
            decisions_tier2_hit: AtomicU64::new(0),
            decisions_tier3_dispatched: AtomicU64::new(0),
            prefetch_total: AtomicU64::new(0),
            prefetch_intent_chat: AtomicU64::new(0),
            prefetch_intent_code: AtomicU64::new(0),
            prefetch_intent_general: AtomicU64::new(0),
            prefetch_bypass: AtomicU64::new(0),
            prefetch_router_calls: AtomicU64::new(0),
            prefetch_router_errors: AtomicU64::new(0),
            prefetch_fallback: AtomicU64::new(0),
            prefetch_router_latency: Histogram::new(1000),
            prefetch_cosine_filtered: AtomicU64::new(0),
            prefetch_devops_heuristic: AtomicU64::new(0),
            prefetch_skill_trigger_hit: AtomicU64::new(0),
        }
    }

    /// Load an atomic counter value.
    fn l(a: &AtomicU64) -> u64 {
        a.load(Ordering::Relaxed)
    }

    /// Snapshot all stats as JSON for the health endpoint.
    pub fn snapshot(&self) -> serde_json::Value {
        let db_lat = self.db_exec_latency.percentiles();
        let gemini_lat = self.gemini_latency.percentiles();
        let ap_lat = self.autopilot_latency.percentiles();
        let db_runs = Self::l(&self.db_exec_runs);
        let db_avg_us = if db_runs > 0 { Self::l(&self.db_exec_total_us) / db_runs } else { 0 };
        let ap_ticks = Self::l(&self.autopilot_ticks);
        let ap_avg = if ap_ticks > 0 { Self::l(&self.autopilot_total_ms) / ap_ticks } else { 0 };

        let ev_pub = Self::l(&self.events_published);
        let ev_ce = Self::l(&self.events_consumed_extraction);
        let ev_cs = Self::l(&self.events_consumed_submit);
        let ev_cd = Self::l(&self.events_consumed_decision);
        let ev_le = Self::l(&self.events_lagged_extraction);
        let ev_ls = Self::l(&self.events_lagged_submit);
        let ev_ld = Self::l(&self.events_lagged_decision);
        let ev_lt = Self::l(&self.events_lagged_total_skipped);
        let gm_req = Self::l(&self.gemini_requests);
        let gm_err = Self::l(&self.gemini_errors);
        let gm_ret = Self::l(&self.gemini_retries);
        let t_disp = Self::l(&self.tasks_dispatched);
        let t_comp = Self::l(&self.tasks_completed);
        let d_t1 = Self::l(&self.decisions_tier1_hit);
        let d_t2 = Self::l(&self.decisions_tier2_hit);
        let d_t3 = Self::l(&self.decisions_tier3_dispatched);

        serde_json::json!({
            "events": {
                "published": ev_pub,
                "consumed": { "extraction": ev_ce, "submit": ev_cs, "decision": ev_cd },
                "lagged": { "extraction": ev_le, "submit": ev_ls, "decision": ev_ld, "total_skipped": ev_lt },
                "estimated_backlog": ev_pub.saturating_sub(ev_ce + ev_cs + ev_cd),
            },
            "db_exec": {
                "runs": db_runs, "avg_us": db_avg_us,
                "p50_us": db_lat.0, "p95_us": db_lat.1, "p99_us": db_lat.2,
            },
            "gemini": {
                "requests": gm_req, "errors": gm_err, "retries": gm_ret,
                "p50_ms": gemini_lat.0 / 1000, "p95_ms": gemini_lat.1 / 1000, "p99_ms": gemini_lat.2 / 1000,
            },
            "autopilot": {
                "ticks": ap_ticks, "avg_ms": ap_avg,
                "p50_ms": ap_lat.0 / 1000, "p95_ms": ap_lat.1 / 1000, "p99_ms": ap_lat.2 / 1000,
            },
            "tasks": { "dispatched": t_disp, "completed": t_comp },
            "decisions": { "tier1_hit": d_t1, "tier2_hit": d_t2, "tier3_dispatched": d_t3 },
            "prefetch": {
                "total": Self::l(&self.prefetch_total),
                "intents": {
                    "chat": Self::l(&self.prefetch_intent_chat),
                    "code": Self::l(&self.prefetch_intent_code),
                    "general": Self::l(&self.prefetch_intent_general),
                },
                "bypass": Self::l(&self.prefetch_bypass),
                "router": {
                    "calls": Self::l(&self.prefetch_router_calls),
                    "errors": Self::l(&self.prefetch_router_errors),
                    "p50_ms": self.prefetch_router_latency.percentiles().0 / 1000,
                    "p95_ms": self.prefetch_router_latency.percentiles().1 / 1000,
                    "p99_ms": self.prefetch_router_latency.percentiles().2 / 1000,
                },
                "fallback": Self::l(&self.prefetch_fallback),
                "cosine_filtered": Self::l(&self.prefetch_cosine_filtered),
                "devops_heuristic": Self::l(&self.prefetch_devops_heuristic),
                "skill_trigger_hit": Self::l(&self.prefetch_skill_trigger_hit),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_empty_returns_zeros() {
        let h = Histogram::new(100);
        assert_eq!(h.percentiles(), (0, 0, 0));
    }

    #[test]
    fn histogram_single_sample() {
        let h = Histogram::new(100);
        h.record(42);
        let (p50, p95, p99) = h.percentiles();
        assert_eq!(p50, 42);
        assert_eq!(p95, 42);
        assert_eq!(p99, 42);
    }

    #[test]
    fn histogram_percentiles_sorted() {
        let h = Histogram::new(100);
        // Record 1..=100
        for i in 1..=100 {
            h.record(i);
        }
        let (p50, p95, p99) = h.percentiles();
        assert_eq!(p50, 51);  // index 50 of sorted 1..=100
        assert_eq!(p95, 96);  // index 95
        assert_eq!(p99, 100); // index 99 (clamped to len-1)
    }

    #[test]
    fn histogram_eviction_at_capacity() {
        let h = Histogram::new(5);
        // Fill to capacity
        for i in 1..=5 {
            h.record(i * 100);
        }
        // Record 6th — should evict oldest (100)
        h.record(600);
        let (p50, _, _) = h.percentiles();
        // Remaining: [200, 300, 400, 500, 600] → p50 = index 2 = 400
        assert_eq!(p50, 400);
    }

    #[test]
    fn histogram_out_of_order_input() {
        let h = Histogram::new(100);
        h.record(500);
        h.record(100);
        h.record(300);
        h.record(200);
        h.record(400);
        let (p50, _, _) = h.percentiles();
        // Sorted: [100, 200, 300, 400, 500] → p50 = index 2 = 300
        assert_eq!(p50, 300);
    }

    #[test]
    fn daemon_stats_snapshot_structure() {
        let stats = DaemonStats::new();
        stats.events_published.store(10, Ordering::Relaxed);
        stats.events_consumed_extraction.store(3, Ordering::Relaxed);
        stats.events_consumed_submit.store(4, Ordering::Relaxed);
        stats.events_consumed_decision.store(2, Ordering::Relaxed);
        stats.db_exec_runs.store(5, Ordering::Relaxed);
        stats.db_exec_total_us.store(5000, Ordering::Relaxed);

        let snap = stats.snapshot();
        assert_eq!(snap["events"]["published"], 10);
        assert_eq!(snap["events"]["estimated_backlog"], 1); // 10 - (3+4+2)
        assert_eq!(snap["db_exec"]["runs"], 5);
        assert_eq!(snap["db_exec"]["avg_us"], 1000); // 5000/5
        assert_eq!(snap["tasks"]["dispatched"], 0);
    }

    #[test]
    fn daemon_stats_snapshot_zero_division_safe() {
        let stats = DaemonStats::new();
        let snap = stats.snapshot();
        // No ticks → avg_ms should be 0, not panic
        assert_eq!(snap["autopilot"]["avg_ms"], 0);
        assert_eq!(snap["db_exec"]["avg_us"], 0);
    }
}
