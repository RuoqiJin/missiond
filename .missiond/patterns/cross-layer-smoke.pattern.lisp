;; Pattern card: cross-layer-smoke
;;
;; Recipe for designing a cross-layer smoke test that pins every layer's
;; invariant at its own boundary instead of one opaque mega-runner. Distilled
;; from wave26-06 (router-readiness-smoke), wave27-06 (router-dispatch-
;; descriptor-smoke), wave28-06 (task-runner-loop-smoke). Each smoke owns a
;; synthetic manifest / fixture that crosses every layer it covers, and each
;; layer reports its own failure near its own checker rather than relying on
;; a single bash script to grep through everyone's output.

(pattern-card cross-layer-smoke
  :schema "missiond.pattern-card.v1"
  :version "v1"
  :purpose "Reproduce the MissionD cross-layer smoke recipe so a regression at any layer (schema / planner / renderer / daemon dry-run / verifier) fails near its OWN checker rather than getting swallowed inside a single mega-runner."
  :summary "Layer-local fixtures + one synthetic manifest that crosses every layer. Each layer's checker carries a pinned smoke fixture (e.g. wave28-06 pins on the manifest checker, plan CLI, daemon dry-run handler, batch verifier all in parallel). Cargo only runs when Rust files actually changed."

  :use-for [wave29-07-runner-efficiency-smoke-v1]

  :recipe
    ["1. Identify every layer the smoke covers BEFORE designing fixtures. wave28-06 covers four: (a) schema layer (check-task-runner-manifest.mjs), (b) plan CLI layer (plan-task-runner.mjs), (c) daemon dry-run layer (mission_plan_task_runner_dry_run handler in plan.rs), (d) batch verifier layer (verify-task-runner-batch.mjs). Each layer gets its own fixture inside its own checker."
     "2. Author ONE synthetic manifest (or descriptor / atlas / receipt) that exercises every invariant the smoke pins. wave28-06 uses a 3-node manifest with A/B/C dispatch groups, productive_only=true, mixed verification tiers — small enough to read in one screen, dense enough to drive every layer once."
     "3. Pin the synthetic fixture into EACH layer's --dry-fixture catalogue with a category tag like 'wave28-06-loop-smoke'. When the smoke fails, every layer's --dry-fixture output names the failing case in its own diagnostics — no need to grep through a single bash script's stdout."
     "4. Add defence-in-depth pinned cases. wave28-06 also pins 'wave28-06-loop-smoke-archive-substring-rejected' to prove that even with a real-looking task id, the productive_only gate fires at the schema layer (first defence), the renderer skip-paths (second), and the verifier (third). Each layer rejects independently; one missed defence does not collapse the whole smoke."
     "5. Pin productive-only, context navigation, preamble-read trace, lineage, receipts, and ready-queue semantics in the SAME synthetic wave. wave29-07 covers the full wave29 surface; wave28-06 covered the full wave28 surface. The smoke wave is the integration point."
     "6. Use smoke-tier acceptance when no Rust files changed in the wave. wave28-06 defaults to verification_tier=smoke and explicitly skips cargo because wave28-01..05 are all Node/Lisp surfaces. Adding cargo for orchestration-only changes wastes ~3 minutes per smoke run with zero added signal."
     "7. Acceptance pipeline runs every layer's --dry-fixture in sequence (not parallel — sequential output is easier to triage), then the synthetic manifest end-to-end through plan -> render -> verify. Final step is git diff --check on the smoke task's write-scope (typically the layer checkers themselves, since pinned fixtures live inside them)."
     "8. Document the layer-by-layer pin map in the smoke task's report :notes. wave28-06's report enumerates: 'Layer A pin = wave28-01 manifest checker case wave28-06-loop-smoke-productive-only-pinned (line 1488), Layer B pin = wave28-02 plan CLI case ..., ...'. This makes regression triage a one-line lookup."]

  :known-good [".missiond/tasks/wave26/wave26-06-router-readiness-smoke-v1.lisp"
               ".missiond/tasks/wave27/wave27-06-router-dispatch-descriptor-smoke-v0.lisp"
               ".missiond/tasks/wave28/wave28-06-task-runner-loop-smoke-v0.lisp"
               ".missiond/tasks/wave28/reports/wave28-06-task-runner-loop-smoke-v0.report.lisp"
               "scripts/check-task-runner-manifest.mjs"]

  :anti-pattern
    ["One opaque bash mega-runner that pipes every checker's stdout into a single grep. When the smoke fails, the operator has no idea which layer broke and resorts to re-running each checker by hand — defeating the smoke's purpose."
     "Running cargo in a smoke wave that touched zero Rust files. Adds 3-5 minutes per CI run, zero added signal, and trains operators to ignore long-running smoke times because 'most of it is cargo'."
     "Skipping the defence-in-depth pin because 'one rejection is enough'. When the smoke regresses six months later because someone refactored the renderer skip-paths, only the defence-in-depth pin will catch it before the productive-only gate quietly stops firing in production."]

  :non-goals
    ["End-to-end full integration. Cross-layer smoke pins SHAPE invariants, not full pipeline outputs. A real end-to-end run with real wave data belongs in a separate periodic CI job."
     "Performance regression detection. Smoke fixtures run on tiny synthetic inputs; they are not the place to measure plan CLI latency or batch verifier throughput."]

  :notes
    "When the next wave adds a new layer (e.g. wave29-05 verification receipts), the cross-layer smoke MUST grow to include that layer's checker. Forgetting to extend the smoke is the most common source of regression slip — wave28-06 grew from a 3-layer smoke to a 4-layer smoke when wave28-04 added the daemon dry-run handler.")
