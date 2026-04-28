;; Wave 35 dispatch-time pattern cards.

(pattern-cards wave35-request-review-packet-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave35

  (card lisp-contract-before-response-shape
    :use-for [wave35-01-mission-request-review-packet-v0]
    :summary "When a runtime response becomes part of the product contract, update V3 Lisp first and make Rust/MCP docs a projection."
    :recipe ["Add the compact review-packet rule under mission_request / unified-entry."
             "Keep the Rust change as a response-shape projection, not a new independent policy."
             "Update the MCP tool description only after the Lisp wording is settled."
             "Use tests to pin response shape, not a broad integration rewrite."]
    :known-good [".missiond/v3/missiond-blueprint.lisp"
                 "crates/missiond-daemon/src/handlers/knowledge/request.rs"])

  (card review-packet-not-auto-approval
    :use-for [wave35-01-mission-request-review-packet-v0]
    :summary "A review packet is a display/decision surface. It must not silently approve intent, approve plan, or dispatch work."
    :recipe ["Derive state from local request artifacts and projection result."
             "Expose allowed_responses and next_action hints."
             "Set execute_allowed=false unless the existing explicit execute path was requested."
             "Leave mission_directive / mission_plan review gates as the approval mechanisms."]
    :known-good ["crates/missiond-daemon/src/handlers/knowledge/request.rs"])

  (card utf8-safe-preview
    :use-for [wave35-01-mission-request-review-packet-v0]
    :summary "Any preview derived from Lisp artifact text must truncate on UTF-8 boundaries."
    :recipe ["Use missiond_core::util::safe_byte_truncate rather than string byte slicing."
             "Add a CJK fixture that would split around an 80-byte boundary."
             "Keep the preview compact; the full artifact remains in the request directory."]
    :known-good ["crates/missiond-core/src/util.rs"]))
