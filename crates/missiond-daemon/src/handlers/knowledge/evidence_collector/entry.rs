use super::*;

/// Builder for a single evidence entry. Every field is optional; callers add
/// only what they have. The resulting JSON object always carries
/// `schema_version`, `source`, and `kind` (default `note` if unspecified).
#[derive(Debug, Clone)]
pub(crate) struct EvidenceEntry {
    source: String,
    kind: String,
    /// Free-form passthrough payload. Merged at the top level so legacy
    /// callers that already shape their own JSON keep working unchanged.
    extra: Map<String, Value>,
    inner_dispatch: Option<Value>,
    verification: Option<Value>,
    git_diff: Option<Value>,
    commit_metadata: Option<Value>,
    execution_events: Vec<EventRef>,
    /// State-transition annotation for DAG node entries (`ready -> succeeded`,
    /// `ready -> failed`, etc.). Optional; callers that don't have one omit it.
    state_transition: Option<String>,
    /// Wave-17 / task 06 — primary event-ref summary surfaced at the top of
    /// the evidence entry (mirrors the leading `EventRef`'s status /
    /// source so audit consumers can pivot without diving into the
    /// `execution_events` array). Set via [`Self::with_primary_event_ref`].
    event_ref_status: Option<String>,
    event_ref_source: Option<String>,
    event_ref_warning: Option<String>,
}

impl EvidenceEntry {
    /// Start a new entry with explicit source + kind. Use the constants in
    /// [`source`] and [`kind`] for the well-known taxonomy; arbitrary strings
    /// are also accepted (the schema is intentionally open).
    pub(crate) fn new(source: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            kind: kind.into(),
            extra: Map::new(),
            inner_dispatch: None,
            verification: None,
            git_diff: None,
            commit_metadata: None,
            execution_events: Vec::new(),
            state_transition: None,
            event_ref_status: None,
            event_ref_source: None,
            event_ref_warning: None,
        }
    }

    /// Inner dispatch summary. Caller passes the already-projected
    /// `tool_result_payload(...)` value (or any JSON), the collector wraps
    /// it under `inner_dispatch` so the legacy `inner_result` /
    /// `inner_error` keys can coexist when callers add them via `with_extra`.
    pub(crate) fn with_inner_dispatch(mut self, summary: Value) -> Self {
        self.inner_dispatch = Some(summary);
        self
    }

    /// Verification commands run (tests / lint / build) — caller supplies a
    /// list of commands and a result summary; we record both verbatim.
    ///
    /// `#[allow(dead_code)]`: future verification-runner step (see
    /// `kind::VERIFICATION` docstring above). Exercised by
    /// `typed_setters_land_under_canonical_keys` test.
    #[allow(dead_code)]
    pub(crate) fn with_verification(
        mut self,
        commands: Vec<String>,
        result_summary: Value,
    ) -> Self {
        self.verification = Some(json!({
            "commands": commands,
            "result_summary": result_summary,
        }));
        self
    }

    /// Git diff snapshot — caller picks whatever shape they have
    /// (`git diff --stat` text, structured per-file list, etc.). Stored
    /// verbatim under `git_diff`.
    ///
    /// `#[allow(dead_code)]`: paired with `with_verification` above; the
    /// verification runner attaches a git-diff snapshot. Exercised by
    /// `typed_setters_land_under_canonical_keys` test.
    #[allow(dead_code)]
    pub(crate) fn with_git_diff(mut self, payload: Value) -> Self {
        self.git_diff = Some(payload);
        self
    }

    /// Commit metadata — `commit_hash` is the resolved sha (40 hex chars or
    /// short form, caller's choice). `commit_status` is a free string (e.g.
    /// `"committed"` / `"detached_head"` / `"dirty"`).
    ///
    /// `#[allow(dead_code)]`: scoped-commit handoff metadata is the
    /// canonical typed slot for the commit_hash/commit_status round-trip
    /// added in wave-12 task-01. Today plan-runner / DAG scheduler do not
    /// produce a scoped commit themselves (they hand off to the caller),
    /// but the typed setter is on the public surface so the upcoming
    /// scoped-commit wiring can drop it in. Exercised by
    /// `commit_metadata_round_trip_via_typed_setter`.
    #[allow(dead_code)]
    pub(crate) fn with_commit(
        mut self,
        commit_hash: Option<String>,
        commit_status: Option<String>,
    ) -> Self {
        let mut m = Map::new();
        if let Some(h) = commit_hash {
            m.insert("commit_hash".to_string(), Value::String(h));
        }
        if let Some(s) = commit_status {
            m.insert("commit_status".to_string(), Value::String(s));
        }
        if !m.is_empty() {
            self.commit_metadata = Some(Value::Object(m));
        }
        self
    }

    /// Append one ExecutionEvent reference. Caller can call this multiple
    /// times. To record "no event available" use `EventRef::unavailable(...)`
    /// rather than skipping the call — that distinction matters to consumers.
    pub(crate) fn add_execution_event(mut self, event: EventRef) -> Self {
        self.execution_events.push(event);
        self
    }

    /// Annotate a DAG node state transition (e.g. `"ready -> succeeded"`).
    pub(crate) fn with_state_transition(mut self, transition: impl Into<String>) -> Self {
        self.state_transition = Some(transition.into());
        self
    }

    /// Wave-17 / task 06 — surface the primary event-ref provenance at the
    /// top level of the evidence entry. Mirrors what `add_execution_event`
    /// would record on the leading `EventRef` so audit consumers can pivot
    /// on `event_ref_status` / `event_ref_source` without iterating the
    /// `execution_events` array.
    ///
    /// `warning` is emitted on the JSON envelope only when present (e.g.
    /// "log query error: …" when the resolver had to fall back from the
    /// log path to `unavailable`).
    pub(crate) fn with_primary_event_ref(
        mut self,
        event_ref: &EventRef,
        warning: Option<String>,
    ) -> Self {
        self.event_ref_status = Some(event_ref.status.as_wire().to_string());
        // Wave-18 / task 01 — `event_ref_source` now carries the resolver
        // provenance (`live | passive_cache | event_log_query | unavailable`)
        // instead of the raw wire source ("execution") so audit consumers
        // can pivot directly on the lookup tier without re-deriving it from
        // the warning string.
        self.event_ref_source = Some(event_ref.provenance.as_wire().to_string());
        self.event_ref_warning = warning.or_else(|| {
            // Surface the unavailable_reason as the warning when no other
            // explicit warning was passed in — keeps the failure surface
            // visible without requiring the caller to duplicate it.
            if matches!(event_ref.status, EventRefStatus::Unavailable) {
                event_ref.unavailable_reason.clone()
            } else {
                None
            }
        });
        self
    }

    /// Drop an arbitrary key/value into the entry. Used for fields not
    /// covered by typed setters (legacy passthrough — `target_tool`,
    /// `dispatch_strategy`, `node_id`, `plan_hint_summary`, etc.).
    pub(crate) fn with_extra(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extra.insert(key.into(), value);
        self
    }

    /// Bulk merge an existing JSON object into `extra`. Useful when a caller
    /// has already built a payload object (e.g. the legacy
    /// `plan_runner_dispatch` shape) and wants to migrate without rewriting
    /// every field by hand.
    ///
    /// `#[allow(dead_code)]`: wave-13 plan.rs / plan_dag.rs migrated to
    /// per-field `with_extra(...)` calls (more explicit + easier to grep
    /// for the legacy passthrough keys). `merge_extra` stays on the public
    /// surface for the next legacy producer that wants to migrate without
    /// rewriting. Exercised by `merge_extra_skips_canonical_stamps`,
    /// `typed_inner_dispatch_wins_over_extra_legacy_inner_dispatch`, and
    /// `commit_metadata_round_trip_via_legacy_merge`.
    #[allow(dead_code)]
    pub(crate) fn merge_extra(mut self, value: Value) -> Self {
        if let Value::Object(m) = value {
            for (k, v) in m {
                // Avoid clobbering canonical fields the caller filled via
                // typed setters. Legacy keys override only if no typed
                // counterpart was set.
                match k.as_str() {
                    "schema_version" | "source" | "kind" | "recorded_at" => continue,
                    _ => {
                        self.extra.insert(k, v);
                    }
                }
            }
        }
        self
    }

    /// Render as a JSON value ready to hand to `append_plan_evidence_entry`.
    /// The `recorded_at` stamp is added by the underlying sidecar writer (so
    /// every entry — legacy or new — gets the same wall-clock semantics);
    /// the builder does NOT pre-stamp it here.
    pub(crate) fn into_json(self) -> Value {
        let Self {
            source,
            kind,
            extra,
            inner_dispatch,
            verification,
            git_diff,
            commit_metadata,
            execution_events,
            state_transition,
            event_ref_status,
            event_ref_source,
            event_ref_warning,
        } = self;

        let mut m = Map::new();
        m.insert(
            "schema_version".to_string(),
            Value::String(EVIDENCE_SCHEMA_VERSION.to_string()),
        );
        m.insert("source".to_string(), Value::String(source));
        m.insert("kind".to_string(), Value::String(kind));
        if let Some(t) = state_transition {
            m.insert("state_transition".to_string(), Value::String(t));
        }
        if let Some(v) = inner_dispatch {
            m.insert("inner_dispatch".to_string(), v);
        }
        if let Some(v) = verification {
            m.insert("verification".to_string(), v);
        }
        if let Some(v) = git_diff {
            m.insert("git_diff".to_string(), v);
        }
        if let Some(v) = commit_metadata {
            m.insert("commit".to_string(), v);
        }
        if let Some(s) = event_ref_status {
            m.insert("event_ref_status".to_string(), Value::String(s));
        }
        if let Some(s) = event_ref_source {
            m.insert("event_ref_source".to_string(), Value::String(s));
        }
        if let Some(w) = event_ref_warning {
            m.insert("event_ref_warning".to_string(), Value::String(w));
        }
        if !execution_events.is_empty() {
            let arr: Vec<Value> = execution_events
                .into_iter()
                .map(EventRef::into_json)
                .collect();
            m.insert("execution_events".to_string(), Value::Array(arr));
        }
        // Merge extra last so the canonical typed keys above always win
        // when both sides set the same field — the typed path is the
        // authoritative one.
        for (k, v) in extra {
            m.entry(k).or_insert(v);
        }
        Value::Object(m)
    }
}
