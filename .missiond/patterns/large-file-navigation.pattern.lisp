;; Pattern card: large-file-navigation
;;
;; Recipe for navigating large source files without burning a 200K context
;; window on whole-file reads. Distilled from working in
;; crates/missiond-daemon/src/handlers/knowledge/plan.rs (~24k lines), the
;; wave28 plan.rs handler refactors, and the wave29 context-atlas which
;; explicitly lists grep anchors per file so workers jump straight to the
;; relevant region.

(pattern-card large-file-navigation
  :schema "missiond.pattern-card.v1"
  :version "v1"
  :purpose "Reproduce the MissionD large-file navigation recipe so a worker reasoning about a 20k+ line hotspot uses ripgrep anchors and partial reads instead of streaming the whole file into context."
  :summary "Atlas-first navigation: read the wave's context atlas anchors before any broad scan; jump to known module boundaries (mod task_runner_dry_run, fn longest_from, fn parse_task_runner_mode); use rg --text or /usr/bin/grep -na when binary detection trips a tool; never place raw NUL bytes in source — escape as \\u0000."

  :use-for [wave29-01-context-atlas-schema-v0
            wave29-06-ready-queue-planner-v0]

  :recipe
    ["1. Read the wave's context atlas FIRST. .missiond/tasks/<wave>/context-atlas.lisp lists per-file :purpose + :grep anchors so the worker knows exactly which symbols / mod boundaries / test names to land on without scanning the whole file. wave29 atlas pins seven anchors for plan.rs alone (mod task_runner_dry_run, parse_task_runner_mode, attach_task_runner_block, compute_runner_block, fn longest_from, task_runner_loop_smoke_pins_wave28_invariants, strip_rust_comments_and_strings)."
     "2. Use ripgrep with line numbers and a tight pattern list before any wholesale read. `rg -n 'fn longest_from|mod task_runner_dry_run|attach_task_runner_block' crates/missiond-daemon/src/handlers/knowledge/plan.rs` lands every relevant region in <500ms with line offsets. Only after rg returns line numbers should the worker fall back to a partial Read with offset/limit."
     "3. For partial reads, target rg-found line numbers with a generous window (±50-100 lines). The partial-read tool's offset/limit covers the surrounding context without dragging the whole file into the model. Most plan.rs functions fit in 100-300 lines around their fn signature."
     "4. When a tool reports binary detection on a text file, escape to /usr/bin/grep -na or rg --text. Generated SQLx cache files and some Lisp manifests with embedded U+0000 escapes occasionally trip tools that auto-detect binary by NUL probing. The --text / -a flags force textual treatment."
     "5. NEVER place raw NUL bytes in source files. When a fixture or schema needs to encode a NUL for round-trip testing, use a textual escape such as \\u0000 (JSON / Rust source) or \\\\x00 (shell). Raw NULs make the file un-greppable for every other worker downstream."
     "6. For Rust files, prefer cargo's symbol output as a navigation index when context-atlas anchors are stale. `cargo doc --no-deps --document-private-items` and `cargo metadata --format-version 1` both surface module boundaries that ripgrep can pin against."
     "7. After landing on the right region, mentally bookmark the nearest mod / fn boundary and reuse it for subsequent reads. Most wave28-29 work in plan.rs returns to mod task_runner_dry_run + the test module 5-10 times during a single task; remembering the line range (~11724 for the mod, ~22900 for the tests) saves a rg call per round trip."
     "8. Update the context atlas when a new anchor proves load-bearing. wave29-01 owns context-atlas-v1; if a future task discovers a new must-know symbol in plan.rs, add it to .missiond/tasks/<wave>/context-atlas.lisp under the file's :grep vector so the next worker inherits the discovery."]

  :known-good ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
               ".missiond/tasks/wave29/context-atlas.lisp"
               ".missiond/tasks/wave29/pattern-cards.lisp"
               ".missiond/claudecode/wave29-shared-preamble.md"]

  :anti-pattern
    ["Reading a 24k-line file end-to-end 'just to get the layout' before any rg pass. Burns 60K+ context on a file the worker may only need 200 lines of, and pushes the model toward summarization instead of citation."
     "Falling back to `cat <file> | head -2000` because the Read tool reported binary. Use rg --text or /usr/bin/grep -na first; the false-positive binary detection is recoverable without a context blowout."
     "Embedding raw NUL bytes in fixture sources to test parser tolerance. Every other worker who tries to read or grep that fixture afterwards will hit the same binary-detection failure — escape NULs textually instead."]

  :non-goals
    ["Rewriting plan.rs to be smaller. The handler module's size reflects its real responsibility surface; splitting it for navigation reasons alone introduces refactor risk without solving the navigation problem (a multi-file split still needs an index)."
     "Replacing ripgrep with a structured AST tool. rg + line offsets + partial reads are the cheap, deterministic, model-friendly path. Tree-sitter / rust-analyzer indices are useful for IDE work but add latency + tooling dependencies that this pattern intentionally avoids."]

  :notes
    "The wave29 context-atlas is the structural fix for large-file navigation churn. Pattern-cards (this card) document the operational discipline; the atlas mechanizes it. When both are present, a worker should read the atlas anchors and follow the pattern card's recipe in tandem.")
