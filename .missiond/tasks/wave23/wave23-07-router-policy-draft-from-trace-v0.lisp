;; Wave 23 task contract.

(task wave23-07-router-policy-draft-from-trace-v0
  :schema "missiond.task-contract.v1"
  :title "Router policy draft from trace v0"
  :kind lisp-only
  :status ready
  :owner "codex-architect"
  :depends-on ["wave23-06-trace-summary-analyzer-v0"]
  :dispatch-strategy "manual"
  :goal "Codex-owned architecture task: add an architecture-only draft for future LLM router policy derived from session traces, without replacing ClaudeCode or changing runtime dispatch."

  :write-scope
    [".missiond/v2/intent-machine-contract.lisp"
     ".missiond/v2/intent-workstation-policy.lisp"
     ".missiond/v2/intent-pillar-source-index.lisp"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/tasks/**"
     ".missiond/claudecode/**"]

  :requirements
    ["Do not delegate this blueprint task to ClaudeCode; Codex owns the architecture edit."
     "Define trace-derived router policy as architecture-designed only."
     "Describe backend classes: claudecode, missiond-llm-router, deterministic-checker, patch-worker, verifier-worker."
     "Record that Wave23 only collects/summarizes trace; it does not replace ClaudeCode."
     "Preserve source-index invariants and do not mark runtime router code-aligned."]

  :acceptance
    ["node scripts/check-architecture-lisp.mjs --all-v2"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-pillar-source-index.lisp"]

  :commit
    (:required true
     :message "docs(v2): draft trace-derived router policy"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Architecture anchors added."
     "Explicit non-goals."
     "Acceptance command results."])
