;; Wave 19 task contract.

(task wave19-04-shared-memory-ledger-v0
  :schema "missiond.task-contract.v1"
  :title "Shared memory ledger v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave19-00-machine-contract-pilot"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Create a Lisp shared-memory ledger shape for parallel ClaudeCode agents: claims, observations, blockers, commits, and handoff pointers."

  :write-scope
    [".missiond/tasks/schema/shared-memory-v1.lisp"
     ".missiond/tasks/wave19/shared-memory.lisp"
     "scripts/check-task-memory.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     "scripts/check-task-contract.mjs"
     "scripts/render-claudecode-task.mjs"]

  :requirements
    ["Define a data-Lisp ledger, not a prose note: entries must be S-expressions with stable heads such as claim, observation, blocker, completion, correction."
     "Add a checker with --dry-fixture; validate task ids, entry ids, timestamps or monotonic sequence numbers, touched files as repo-relative paths, and no duplicate entry ids."
     "Seed .missiond/tasks/wave19/shared-memory.lisp with a header and zero or one bootstrap observation; do not log fake task completions."
     "Document in the schema that agents append entries only inside their claimed write-scope, while the ledger itself is the one shared write target for coordination."]

  :acceptance
    ["node scripts/check-task-memory.mjs --dry-fixture"
     "node scripts/check-task-memory.mjs .missiond/tasks/wave19/shared-memory.lisp"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/shared-memory-v1.lisp .missiond/tasks/wave19/shared-memory.lisp scripts/check-task-memory.mjs"]

  :commit
    (:required true
     :message "feat(tasks): add shared memory ledger contract"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Ledger entry types."
     "Checker dry-fixture coverage."
     "Acceptance command results."])
