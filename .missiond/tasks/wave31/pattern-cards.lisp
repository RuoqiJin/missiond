;; Wave 31 dispatch-time pattern cards.
;; Read-only guidance for workers. Stable cards remain under .missiond/patterns/.

(pattern-cards wave31-request-entry
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave31

  (card request-local-projection
    :use-for [wave31-01-mission-request-local-projections-v0]
    :recipe ["Keep mission_request conservative: no auto-approval, no direct workstation dispatch, no DB schema migration."
             "After unified_entry returns, parse the inner JSON payload and project only stable Lisp sexp keys into request-local artifacts."
             "Directive stage writes .missiond/requests/<request_id>/intent-alignment.lisp from compiled_sexp or compiled_sexp_preview."
             "Plan compile stage writes .missiond/requests/<request_id>/plan.lisp from compiled_sexp or compiled_sexp_preview."
             "Use atomic_write_artifact and request_paths; status should expose request-local artifact existence/paths without reading arbitrary files."
             "Unit-test the pure projection/extraction helpers without constructing AppState; run cargo tests for the request module."]
    :known-good ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
                 "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
                 "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
                 "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                 "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"]))
