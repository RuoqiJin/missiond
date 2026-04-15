;; MissionD — Pillar: semantic-parser
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

  (pillar semantic-parser
    (purpose "multi-layer recognizer: raw PTY screen → structured states")

    ;; commit 5a5f805: missiond-semantic EXTRACTED → standalone open-source crate
    ;; All targets below are in semantic-terminal crate (external workspace dep)
    ;; Forge GenGap (pure_parsing/) moved to missiond-core/src/semantic_parsing/
    (component parser-pipeline
      :target "semantic-terminal/src/ (external crate)"
      (pipeline
        pattern-config -> fingerprint-registry -> state-parser
        -> confirm-parser -> tool-output-parser)
      (shared-resource "Arc<CompiledPatterns> from YAML hot-reload"))

    (component pattern-config
      :target "semantic-terminal/src/patterns.rs"
      (dispatch "CliEngine enum → engine-specific YAML + parser"))

    (component claude-code-parser
      :target "semantic-terminal/src/state.rs"
      (detection-order
        trust-dialog -> confirm-dialog -> idle-or-slash
        -> processing -> responding -> error))

    (component gemini-parser
      :target "semantic-terminal/src/gemini_state.rs"
      (detection-order
        error -> thinking -> responding -> tool-running
        -> idle -> idle-placeholder -> idle-transitional))

    (component fingerprint :target "semantic-terminal/src/fingerprint.rs")
    (component confirm     :target "semantic-terminal/src/confirm.rs")
    (component tool-parser :target "semantic-terminal/src/tool.rs")
    (component status      :target "semantic-terminal/src/status.rs")
    (component title       :target "semantic-terminal/src/title.rs")

    (component semantic-parsing-gengap
      :target "crates/missiond-core/src/semantic_parsing/"
      :note "Forge GenGap (generated.rs + custom.rs + mod.rs) — moved here from former missiond-semantic"))

