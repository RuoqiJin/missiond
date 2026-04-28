;; Wave 31 dispatch plan.
;; This wave intentionally has one ClaudeCode worker task so we can measure
;; post-wave30/v3-entry efficiency without parallel noise.

(dispatch-plan wave31
  :schema "missiond.dispatch-plan.v0"
  :policy "productive-only"
  :shared-preamble ".missiond/claudecode/wave31-shared-preamble.md"
  :brief-mode thin
  :mainline "Move mission_request from request.lisp + compat pipeline into request-local Lisp projections."
  :measurement-goal "Observe ClaudeCode behavior after V3 blueprint compression, request entry registration, lifecycle finalization, and hard/soft dependency upgrades."

  :nodes
    [(node wave31-01-mission-request-local-projections-v0
       :group A
       :verification-tier local
       :estimated-minutes 45
       :write-scope ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
                     "crates/missiond-mcp/src/tools/knowledge/request.rs"
                     ".missiond/v3/missiond-blueprint.lisp"])])
