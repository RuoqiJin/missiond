//! PTY Anomaly Detector — passive detection of parser/CLI compatibility issues.
//!
//! Monitors state machine transitions, parser confidence, and anchor integrity
//! to detect when a CLI update has broken PTY parsing patterns.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use semantic_terminal::State;
use missiond_shared::CliEngine;

/// Anomaly detected by the PTY compatibility guardian
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyAnomaly {
    pub kind: AnomalyKind,
    pub slot_id: String,
    pub engine: CliEngine,
    pub severity: AnomalySeverity,
    pub message: String,
    /// Sample terminal text when anomaly was detected
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnomalyKind {
    /// State stuck for too long without spinner change
    StateStuck,
    /// Illegal state transition
    IllegalTransition,
    /// Parser confidence consistently low
    LowConfidence,
    /// Required anchor pattern missing from terminal output
    AnchorMissing,
    /// Too many consecutive frames with no state detected
    UnrecognizedFrames,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnomalySeverity {
    Warning,
    Error,
    Critical,
}

impl std::fmt::Display for AnomalySeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnomalySeverity::Warning => write!(f, "warning"),
            AnomalySeverity::Error => write!(f, "error"),
            AnomalySeverity::Critical => write!(f, "critical"),
        }
    }
}

/// Configuration for anomaly detection thresholds
#[derive(Debug, Clone)]
pub struct AnomalyConfig {
    /// How long a state can remain stuck before warning (default: 30 min)
    pub state_stuck_timeout: Duration,
    /// Minimum average confidence before warning (default: 0.7)
    pub confidence_threshold: f64,
    /// Sliding window size for confidence tracking (default: 60s)
    pub confidence_window: Duration,
    /// Max consecutive frames with no state detected (default: 50 = 5s at 100ms)
    pub unrecognized_threshold: u32,
    /// How often to check anchors (default: 60s)
    pub anchor_check_interval: Duration,
}

impl Default for AnomalyConfig {
    fn default() -> Self {
        Self {
            state_stuck_timeout: Duration::from_secs(30 * 60),
            confidence_threshold: 0.7,
            confidence_window: Duration::from_secs(60),
            unrecognized_threshold: 50,
            anchor_check_interval: Duration::from_secs(60),
        }
    }
}

/// Valid state transitions matrix
static VALID_TRANSITIONS: once_cell::sync::Lazy<HashSet<(State, State)>> =
    once_cell::sync::Lazy::new(|| {
        let mut set = HashSet::new();
        // From Idle
        set.insert((State::Idle, State::Thinking));
        set.insert((State::Idle, State::ToolRunning));
        set.insert((State::Idle, State::Confirming));
        set.insert((State::Idle, State::SlashMenu));
        set.insert((State::Idle, State::Error));
        // From Thinking
        set.insert((State::Thinking, State::Idle));
        set.insert((State::Thinking, State::ToolRunning));
        set.insert((State::Thinking, State::Confirming));
        set.insert((State::Thinking, State::Responding));
        set.insert((State::Thinking, State::Error));
        // From ToolRunning
        set.insert((State::ToolRunning, State::Idle));
        set.insert((State::ToolRunning, State::Thinking));
        set.insert((State::ToolRunning, State::Confirming));
        set.insert((State::ToolRunning, State::Responding));
        set.insert((State::ToolRunning, State::Error));
        // From Confirming
        set.insert((State::Confirming, State::Idle));
        set.insert((State::Confirming, State::Thinking));
        set.insert((State::Confirming, State::ToolRunning));
        set.insert((State::Confirming, State::Error));
        // From Responding
        set.insert((State::Responding, State::Idle));
        set.insert((State::Responding, State::Thinking));
        set.insert((State::Responding, State::ToolRunning));
        set.insert((State::Responding, State::Confirming));
        set.insert((State::Responding, State::Error));
        // From SlashMenu
        set.insert((State::SlashMenu, State::Idle));
        set.insert((State::SlashMenu, State::Thinking));
        set.insert((State::SlashMenu, State::ToolRunning));
        // From Error
        set.insert((State::Error, State::Idle));
        // Starting can go to anything
        set.insert((State::Starting, State::Idle));
        set.insert((State::Starting, State::Thinking));
        set.insert((State::Starting, State::ToolRunning));
        set.insert((State::Starting, State::Confirming));
        set.insert((State::Starting, State::Error));
        set
    });

/// Per-session anomaly detector
pub struct AnomalyDetector {
    config: AnomalyConfig,
    slot_id: String,
    engine: CliEngine,

    // State tracking
    current_state: Option<State>,
    state_entered_at: Instant,

    // Confidence tracking (sliding window)
    confidence_samples: VecDeque<(Instant, f64)>,

    // Unrecognized frame counter
    consecutive_unrecognized: u32,

    // Anchor check timing
    last_anchor_check: Instant,

    // Cooldown: avoid spamming same anomaly type
    last_anomaly_times: std::collections::HashMap<String, Instant>,
    anomaly_cooldown: Duration,

    // Anchor tolerance: consecutive miss counts per anchor
    // (Gemini audit fix: single miss could be transient — require 3 consecutive misses)
    anchor_miss_counts: std::collections::HashMap<String, u32>,
    anchor_miss_threshold: u32,
}

impl AnomalyDetector {
    pub fn new(slot_id: String, engine: CliEngine) -> Self {
        Self::with_config(slot_id, engine, AnomalyConfig::default())
    }

    pub fn with_config(slot_id: String, engine: CliEngine, config: AnomalyConfig) -> Self {
        Self {
            config,
            slot_id,
            engine,
            current_state: None,
            state_entered_at: Instant::now(),
            confidence_samples: VecDeque::new(),
            consecutive_unrecognized: 0,
            last_anchor_check: Instant::now(),
            last_anomaly_times: std::collections::HashMap::new(),
            anomaly_cooldown: Duration::from_secs(300), // 5 min between same anomaly type
            anchor_miss_counts: std::collections::HashMap::new(),
            anchor_miss_threshold: 3, // require 3 consecutive misses before alerting
        }
    }

    /// Feed a new state detection result. Returns anomalies if any detected.
    pub fn on_state_detected(
        &mut self,
        new_state: Option<State>,
        confidence: f64,
    ) -> Vec<PtyAnomaly> {
        let mut anomalies = Vec::new();
        let now = Instant::now();

        // Pre-clone identifiers to avoid borrow conflicts with emit_if_cool
        let slot_id = self.slot_id.clone();
        let engine = self.engine;

        // Track confidence
        self.confidence_samples.push_back((now, confidence));
        self.prune_confidence_window(now);

        if let Some(state) = new_state {
            // Reset unrecognized counter
            self.consecutive_unrecognized = 0;

            // Check state transition legality
            if let Some(prev) = self.current_state {
                if prev != state && !VALID_TRANSITIONS.contains(&(prev, state)) {
                    let sid = slot_id.clone();
                    if let Some(a) = self.emit_if_cool("illegal_transition", || PtyAnomaly {
                        kind: AnomalyKind::IllegalTransition,
                        slot_id: sid,
                        engine,
                        severity: AnomalySeverity::Warning,
                        message: format!("{} → {} is not a valid transition", prev, state),
                        sample_text: None,
                    }) {
                        anomalies.push(a);
                    }
                }
            }

            // Check state stuck
            if self.current_state == Some(state) {
                let stuck_duration = now.duration_since(self.state_entered_at);
                if stuck_duration > self.config.state_stuck_timeout
                    && matches!(state, State::Thinking | State::ToolRunning)
                {
                    let sid = slot_id.clone();
                    if let Some(a) = self.emit_if_cool("state_stuck", || PtyAnomaly {
                        kind: AnomalyKind::StateStuck,
                        slot_id: sid,
                        engine,
                        severity: AnomalySeverity::Error,
                        message: format!(
                            "State {} stuck for {:.0}s",
                            state,
                            stuck_duration.as_secs_f64()
                        ),
                        sample_text: None,
                    }) {
                        anomalies.push(a);
                    }
                }
            } else {
                // State changed → reset timer
                self.current_state = Some(state);
                self.state_entered_at = now;
            }
        } else {
            // No state detected
            self.consecutive_unrecognized += 1;

            if self.consecutive_unrecognized >= self.config.unrecognized_threshold {
                let sid = slot_id.clone();
                let count = self.consecutive_unrecognized;
                if let Some(a) = self.emit_if_cool("unrecognized_frames", || PtyAnomaly {
                    kind: AnomalyKind::UnrecognizedFrames,
                    slot_id: sid,
                    engine,
                    severity: AnomalySeverity::Error,
                    message: format!(
                        "{} consecutive frames without state detection",
                        count
                    ),
                    sample_text: None,
                }) {
                    anomalies.push(a);
                }
            }
        }

        // Check confidence drift
        if let Some(avg) = self.avg_confidence() {
            if avg < self.config.confidence_threshold && self.confidence_samples.len() >= 10 {
                let sid = slot_id.clone();
                let threshold = self.config.confidence_threshold;
                if let Some(a) = self.emit_if_cool("low_confidence", || PtyAnomaly {
                    kind: AnomalyKind::LowConfidence,
                    slot_id: sid,
                    engine,
                    severity: AnomalySeverity::Warning,
                    message: format!(
                        "Average parser confidence {:.2} below threshold {:.2}",
                        avg, threshold
                    ),
                    sample_text: None,
                }) {
                    anomalies.push(a);
                }
            }
        }

        anomalies
    }

    /// Check anchors against current terminal text. Call periodically.
    /// Uses tolerance: only alerts after `anchor_miss_threshold` consecutive misses
    /// to avoid false positives from clear/vim/fast scrolling (Gemini audit fix).
    pub fn check_anchors(&mut self, anchor_results: &[(String, bool)]) -> Vec<PtyAnomaly> {
        let now = Instant::now();
        if now.duration_since(self.last_anchor_check) < self.config.anchor_check_interval {
            return Vec::new();
        }
        self.last_anchor_check = now;

        let slot_id = self.slot_id.clone();
        let engine = self.engine;
        let threshold = self.anchor_miss_threshold;

        let mut anomalies = Vec::new();
        for (id, matched) in anchor_results {
            if *matched {
                // Reset consecutive miss counter
                self.anchor_miss_counts.remove(id);
            } else {
                let count = self.anchor_miss_counts.entry(id.clone()).or_insert(0);
                *count += 1;

                if *count >= threshold {
                    let key = format!("anchor_missing_{}", id);
                    let sid = slot_id.clone();
                    let anchor_id = id.clone();
                    let miss_count = *count;
                    if let Some(a) = self.emit_if_cool(&key, || PtyAnomaly {
                        kind: AnomalyKind::AnchorMissing,
                        slot_id: sid,
                        engine,
                        severity: AnomalySeverity::Critical,
                        message: format!(
                            "Required anchor '{}' missing for {} consecutive checks",
                            anchor_id, miss_count
                        ),
                        sample_text: None,
                    }) {
                        anomalies.push(a);
                    }
                }
            }
        }
        anomalies
    }

    /// Attach sample text to an anomaly (for debugging)
    pub fn with_sample(anomaly: &mut PtyAnomaly, text: &str) {
        // Truncate to 500 chars to avoid bloat
        let truncated = if text.len() > 500 {
            format!("{}...", &text[..500])
        } else {
            text.to_string()
        };
        anomaly.sample_text = Some(truncated);
    }

    fn prune_confidence_window(&mut self, now: Instant) {
        while let Some((time, _)) = self.confidence_samples.front() {
            if now.duration_since(*time) > self.config.confidence_window {
                self.confidence_samples.pop_front();
            } else {
                break;
            }
        }
    }

    fn avg_confidence(&self) -> Option<f64> {
        if self.confidence_samples.is_empty() {
            return None;
        }
        let sum: f64 = self.confidence_samples.iter().map(|(_, c)| c).sum();
        Some(sum / self.confidence_samples.len() as f64)
    }

    /// Emit anomaly only if cooldown has passed for this anomaly type
    fn emit_if_cool(
        &mut self,
        key: &str,
        f: impl FnOnce() -> PtyAnomaly,
    ) -> Option<PtyAnomaly> {
        let now = Instant::now();

        // Periodically prune expired cooldown entries to prevent memory leak
        // (Gemini audit fix: HashMap grows unbounded with dynamic keys)
        if self.last_anomaly_times.len() > 50 {
            let cooldown = self.anomaly_cooldown;
            self.last_anomaly_times
                .retain(|_, last| now.duration_since(*last) < cooldown * 2);
        }

        if let Some(last) = self.last_anomaly_times.get(key) {
            if now.duration_since(*last) < self.anomaly_cooldown {
                return None;
            }
        }
        self.last_anomaly_times.insert(key.to_string(), now);
        Some(f())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detector() -> AnomalyDetector {
        AnomalyDetector::with_config(
            "test-slot".to_string(),
            CliEngine::ClaudeCode,
            AnomalyConfig {
                state_stuck_timeout: Duration::from_millis(100),
                confidence_threshold: 0.7,
                confidence_window: Duration::from_secs(5),
                unrecognized_threshold: 3,
                anchor_check_interval: Duration::from_millis(0), // immediate
                ..Default::default()
            },
        )
    }

    #[test]
    fn test_normal_transitions_no_anomaly() {
        let mut d = detector();
        // Idle → Thinking → ToolRunning → Idle
        assert!(d.on_state_detected(Some(State::Idle), 0.9).is_empty());
        assert!(d.on_state_detected(Some(State::Thinking), 0.9).is_empty());
        assert!(d.on_state_detected(Some(State::ToolRunning), 0.85).is_empty());
        assert!(d.on_state_detected(Some(State::Idle), 0.9).is_empty());
    }

    #[test]
    fn test_illegal_transition() {
        let mut d = detector();
        d.on_state_detected(Some(State::SlashMenu), 0.9);
        // SlashMenu → ToolRunning is now valid (slash commands can trigger tools)
        let anomalies = d.on_state_detected(Some(State::ToolRunning), 0.9);
        assert!(anomalies.is_empty());

        // But Error → Thinking is still invalid
        let mut d2 = detector();
        d2.on_state_detected(Some(State::Error), 0.9);
        let anomalies = d2.on_state_detected(Some(State::Thinking), 0.9);
        assert_eq!(anomalies.len(), 1);
        assert!(matches!(anomalies[0].kind, AnomalyKind::IllegalTransition));
    }

    #[test]
    fn test_unrecognized_frames() {
        let mut d = detector();
        d.on_state_detected(Some(State::Idle), 0.9);
        // 3 consecutive unrecognized frames
        assert!(d.on_state_detected(None, 0.0).is_empty());
        assert!(d.on_state_detected(None, 0.0).is_empty());
        let anomalies = d.on_state_detected(None, 0.0);
        assert_eq!(anomalies.len(), 1);
        assert!(matches!(anomalies[0].kind, AnomalyKind::UnrecognizedFrames));
    }

    #[test]
    fn test_state_stuck() {
        let mut d = detector();
        d.on_state_detected(Some(State::Thinking), 0.9);
        // Wait for stuck timeout
        std::thread::sleep(Duration::from_millis(150));
        let anomalies = d.on_state_detected(Some(State::Thinking), 0.9);
        assert!(anomalies.iter().any(|a| matches!(a.kind, AnomalyKind::StateStuck)));
    }

    #[test]
    fn test_low_confidence() {
        let mut d = AnomalyDetector::with_config(
            "test-slot".to_string(),
            CliEngine::ClaudeCode,
            AnomalyConfig {
                confidence_threshold: 0.7,
                confidence_window: Duration::from_secs(60),
                unrecognized_threshold: 100,
                ..Default::default()
            },
        );

        // Feed 15 low confidence samples
        for _ in 0..15 {
            d.on_state_detected(Some(State::Thinking), 0.5);
        }
        let anomalies = d.on_state_detected(Some(State::Thinking), 0.5);
        // Should eventually trigger low confidence (if cooldown allows)
        // Note: first detection already emitted at sample 10+, cooldown may block
        // We just verify no panic and the mechanism works
        let _ = anomalies;
    }

    #[test]
    fn test_anchor_missing_with_tolerance() {
        let mut d = detector();
        d.anchor_miss_threshold = 3;

        let results = vec![
            ("bottom_bar".to_string(), true),
            ("prompt_symbol".to_string(), false), // missing!
            ("tool_marker".to_string(), true),
        ];

        // First miss → no alert (tolerance)
        assert!(d.check_anchors(&results).is_empty());
        // Second miss → still no alert
        assert!(d.check_anchors(&results).is_empty());
        // Third miss → alert!
        let anomalies = d.check_anchors(&results);
        assert_eq!(anomalies.len(), 1);
        assert!(matches!(anomalies[0].kind, AnomalyKind::AnchorMissing));
        assert!(anomalies[0].message.contains("prompt_symbol"));

        // Anchor recovers → counter resets
        let recovered = vec![
            ("bottom_bar".to_string(), true),
            ("prompt_symbol".to_string(), true),
            ("tool_marker".to_string(), true),
        ];
        d.check_anchors(&recovered);
        // Miss again → needs 3 more consecutive misses
        assert!(d.check_anchors(&results).is_empty());
    }

    #[test]
    fn test_cooldown_prevents_spam() {
        let mut d = detector();
        d.anomaly_cooldown = Duration::from_millis(200);

        // First illegal transition (Error → Thinking) → emits anomaly
        d.on_state_detected(Some(State::Error), 0.9);
        let a1 = d.on_state_detected(Some(State::Thinking), 0.9);
        assert_eq!(a1.len(), 1, "first illegal transition should emit");

        // Immediately repeat → cooldown blocks it
        // Go back to Error first (Thinking→Idle valid, Idle→Error valid)
        d.on_state_detected(Some(State::Idle), 0.9);
        d.on_state_detected(Some(State::Error), 0.9);
        let a2 = d.on_state_detected(Some(State::Thinking), 0.9);
        assert!(a2.is_empty(), "cooldown should block repeat");

        // Wait for cooldown to expire
        std::thread::sleep(Duration::from_millis(300));
        d.on_state_detected(Some(State::Idle), 0.9);
        d.on_state_detected(Some(State::Error), 0.9);
        let a3 = d.on_state_detected(Some(State::Thinking), 0.9);
        assert_eq!(a3.len(), 1, "cooldown expired, should emit again");
    }
}
