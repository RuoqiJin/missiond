(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260602204120-unscoped-evidence-read-model-guard"
  :events ((event created
             :at "2026-06-02T20:41:20Z"
             :actor codex
             :summary "Created after live smoke showed unscoped conversation_audit mixing project-scoped Payments/ASR support_refs from persisted evidence_items.")
           (event verification
             :at "2026-06-02T20:44:57Z"
             :actor codex
             :summary "Implemented non-full_debug unscoped evidence read-model skip with scope_skipped diagnostics. Verified with rustfmt, targeted context_gather tests, cargo check -p missiond-daemon, V3 contract check, memory/KB isomorphism checker including dry fixture, runtime path hygiene, source hygiene, deployment closure plane checker, and git diff --check.")))
