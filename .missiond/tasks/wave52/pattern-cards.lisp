;; Wave 52 dispatch-time pattern cards.

(pattern-cards wave52-contract-artifact-validation-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave52

  (card commit-snapshot-artifact-validation
    :use-for [wave52-01-contract-artifact-validation-v0]
    :summary "When verifying a historical commit, validate artifact bytes from that commit rather than the current worktree."
    :recipe ["Discover known Lisp artifact paths from the resolved commit's changed file list."
             "Read each artifact with git show <commit>:<path> and materialize it into a temporary path preserving the basename or relative path expected by the checker."
             "Run the existing checker script against the temporary file; do not duplicate schemas inside verify-task-contract."
             "Surface checker stderr/stdout as verifier errors that name the artifact path and checker."]
    :known-good ["--commit=7f462d17 sees the invalid wave51 session-trace as it existed in that commit"
                 "later parent repairs in HEAD do not hide the worker-commit defect"])

  (card pure-core-plus-cli-effects
    :use-for [wave52-01-contract-artifact-validation-v0]
    :summary "Keep imported verifier helpers pure; put artifact I/O behind explicit CLI/helper boundaries."
    :recipe ["Do not add disk, git, or child-process side effects to verifyContract(contract, commitInfo)."
             "Add a separate exported discovery/helper if useful, with explicit inputs."
             "Dry fixtures should cover discovery/plan logic and at least one mocked artifact failure."
             "The live negative regression can cover the real checker execution path."]
    :known-good ["verify-task-run.mjs can still import verifyContract without hidden I/O"
                 "task-scope-guard commit mode continues to delegate to the CLI"]))
