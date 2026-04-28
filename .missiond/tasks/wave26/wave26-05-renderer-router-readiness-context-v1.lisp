;; Wave 26 task contract.

(task wave26-05-renderer-router-readiness-context-v1
  :schema "missiond.task-contract.v1"
  :title "Renderer router readiness context v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave26-01-router-backend-registry-v0" "wave26-02-router-recommendation-readiness-v1" "wave26-04-report-router-readiness-fields-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Render backend-readiness context and advisory commands into ClaudeCode task briefs without making Markdown load-bearing or changing backend dispatch."

  :write-scope
    ["scripts/render-claudecode-task.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"]

  :must-not-touch
    ["crates/**"
     "scripts/check-task-report.mjs"
     "scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"
     ".missiond/tasks/schema/report-contract-v1.lisp"
     ".missiond/router/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave25/**"
     ".missiond/tasks/wave26/wave26-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Add optional task-contract field :router-backend-registry-path / :router_backend_registry_path, documented as advisory-only."
     "Renderer should auto-detect .missiond/router/router-backend-registry-v1.lisp when present, analogous to router-policy auto-detect."
     "Router Policy section should include a read-only check-router-backend-registry command and should add --backend-registry <path> to the recommend-task-backend command when registry path resolves."
     "Report Contract section should mention the Wave26 readiness fields using MAY-language only."
     "Section must keep literal 'advisory' and 'dry-run only' wording and must explicitly say backend dispatch MUST NOT be switched based on rendered text."
     "Renderer must not shell out; commands are text only."]

  :acceptance
    ["node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave26/wave26-02-router-recommendation-readiness-v1.lisp > /tmp/wave26-router-readiness.md"
     "rg \"Router Policy|router-backend-registry|--backend-registry|advisory|dry-run only\" /tmp/wave26-router-readiness.md"
     "node scripts/check-task-contract.mjs --all"
     "node scripts/check-task-report.mjs --dry-fixture"
     "git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): render router readiness context"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Rendered section changes."
     "Proof renderer remains non-executing."
     "Acceptance command results."])

