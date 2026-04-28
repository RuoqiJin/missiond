;; Pattern card: node-cli-readonly
;;
;; Recipe for adding a new READ-ONLY MissionD Node CLI under scripts/. Distilled
;; from scripts/plan-task-runner.mjs (wave28-02), scripts/render-wave-briefs.mjs
;; (wave28-03), scripts/verify-task-runner-batch.mjs (wave28-05). These CLIs are
;; planners / renderers / verifiers — they READ Lisp and emit deterministic
;; output, never dispatch / spawn / git mutate / call the network or an LLM.

(pattern-card node-cli-readonly
  :schema "missiond.pattern-card.v1"
  :version "v1"
  :purpose "Reproduce the MissionD read-only Node CLI recipe so a new planner / renderer / verifier ships with the same purity guarantees, deterministic output, and named-export surface."
  :summary "scripts/<name>.mjs that reads Lisp via scripts/lib/missiond_lisp.mjs, projects through wave28-01 named exports, emits sorted JSON or Lisp output, runs in-process fixtures with --dry-fixture, and never shells out / never spawns / never touches git / network / LLM."

  :use-for [wave29-03-runner-wave-prep-v0
            wave29-06-ready-queue-planner-v0]

  :recipe
    ["1. Make read/write effects explicit in the CLI usage string. The first paragraph of usage MUST state 'reads X, never writes anything outside Y' so reviewers can verify purity without reading the source."
     "2. Reuse named exports from existing checkers instead of re-implementing schema rules. wave28-02 (plan CLI) imports projectManifest / readManifestFile / validateManifestObject / VERIFICATION_TIERS / OVERLAP_POLICIES / SCHEMA from scripts/check-task-runner-manifest.mjs — single source of truth for shape rules."
     "3. Make output byte-stable. Sort node ids at every batch construction site; sort object keys via a recursive sortKeys() helper before JSON.stringify; sort diagnostic arrays by (severity, file, line); sort fixture catalogue output by name. Determinism fixtures should re-run the CLI on a permuted input and assert byte-identical output."
     "4. Expose pure functions as named exports for fixture and downstream reuse: <verb>FromFile(path, opts?) for the on-disk entry, <verb>FromObject(record, opts?) for the in-memory entry, plus the schema constants the CLI consumes. Gate main() on `import.meta.url === \\`file://${process.argv[1]}\\`` so importers do not trigger CLI side-effects."
     "5. Self-audit purity before commit by grepping the source: rg -nE 'child_process|spawn|fetch|http|https|exec|git|openai|anthropic' scripts/<name>.mjs MUST return ZERO matches when the task claims read-only planning. Document this guarantee in the report's :notes."
     "6. Ship --dry-fixture with both pass and fail cases covering the CLI's invariants (e.g. determinism, cycle detection, missing-input shape, schema short-circuit). Ship --json (default for tooling) and --lisp (alt format for piping to a future verifier) where applicable. The wave28-02 plan CLI ships 12 cases across 11 categories."
     "7. When the CLI optionally touches disk (e.g. on-disk join), gate that pass behind an `opts.<flag>:boolean` so fixtures can opt out. wave28-02's `checkTaskContractsOnDisk` is the canonical example — fixtures default it false so they need no on-disk dependencies."
     "8. Acceptance pipeline mirrors the schema-checker card: --dry-fixture (zero failures) -> end-to-end smoke against a real input file -> check-task-contract.mjs --all (no regressions) -> git diff --check (whitespace clean) -> task-scope-guard --mode staged (only declared paths staged)."]

  :known-good ["scripts/plan-task-runner.mjs"
               "scripts/render-wave-briefs.mjs"
               "scripts/verify-task-runner-batch.mjs"
               "scripts/render-claudecode-task.mjs"
               "scripts/check-task-runner-manifest.mjs"]

  :anti-pattern
    ["Calling child_process.spawn / exec / git from inside the CLI to 'just check one thing' — once the CLI shells out it stops being read-only and the task-scope-guard / pre-commit hooks lose their purity guarantee."
     "Re-implementing schema rules locally because importing the named exports is one line longer. This is how schema drift happens: the wave28-01 manifest checker rejects an enum value but the wave28-02 planner accepts it because someone copied an old version of the enum set."
     "Producing output whose key order depends on JS object insertion. The verifier downstream byte-compares; non-deterministic output flakes CI even when the underlying logic is correct."]

  :non-goals
    ["Owning the schema. Read-only CLIs CONSUME schema rules from existing checkers; they do not introduce new validation rules. New rules belong in the schema layer (schema-checker pattern)."
     "Mutating any file the user did not name on the command line. Read-only CLIs may emit to stdout / stderr but MUST NOT write to disk in their canonical mode (--write subcommands, when present, are explicit opt-ins documented in usage)."]

  :notes
    "scripts/plan-task-runner.mjs is the densest exemplar — 12 fixtures, named exports for daemon dry-run + batch verifier, sortKeys + per-array sort, opt-in on-disk join, gated main(). When designing a new read-only CLI, read its end-to-end pipeline first.")
