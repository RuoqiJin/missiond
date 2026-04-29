;; Wave 44 dispatch-time pattern cards.

(pattern-cards wave44-v3-request-local-artifact-roots-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave44

  (card lisp-first-gap-fix
    :use-for [wave44-01-request-local-artifact-roots-v0]
    :summary "When behavior and Lisp disagree, update the Lisp contract first, then make code and checker match it."
    :recipe ["Change .missiond/v3/missiond-blueprint.lisp before Rust or JS."
             "Keep the high-level invariant short: request-local artifacts are SSOT; compatibility roots are opt-in projections."
             "Then adjust request.rs, MCP schema, and smoke checks to enforce the invariant."]
    :known-good [".missiond/v3/missiond-blueprint.lisp"
                 "scripts/check-v3-code-isomorphism-complete.mjs"])

  (card request-local-default
    :use-for [wave44-01-request-local-artifact-roots-v0]
    :summary "The unified request surface should not teach callers to inspect legacy artifact roots."
    :recipe ["The live smoke should pass write_request_file=true but omit write_file unless explicitly testing legacy compatibility."
             "Assert request.lisp, intent-alignment.lisp, and plan.lisp under .missiond/requests/<request_id>."
             "Assert no .missiond/alignment/<request_id>/ and no .missiond/plans/*/PLAN.lisp containing the smoke objective are created by default."
             "Leave DB audit rows alone; the filesystem invariant is about artifact roots, not database cleanup."]
    :known-good ["scripts/check-v3-request-flow-smoke.mjs"
                 "crates/missiond-daemon/src/handlers/knowledge/request.rs"])

  (card compat-opt-in-alias
    :use-for [wave44-01-request-local-artifact-roots-v0]
    :summary "Avoid breaking old callers while giving the V3 path a clearer switch name."
    :recipe ["Prefer a new compat_write_file boolean as the V3 name."
             "Preserve write_file=true as a legacy alias for compatibility writers."
             "Do not let compat_write_file affect request-local request.lisp/intent-alignment.lisp/plan.lisp projection."
             "If adding --compat-write-file to the smoke, keep it opt-in and outside check-v3-code-isomorphism-complete."]
    :known-good ["crates/missiond-mcp/src/tools/knowledge/request.rs"
                 "crates/missiond-daemon/src/handlers/knowledge/request.rs"]))
