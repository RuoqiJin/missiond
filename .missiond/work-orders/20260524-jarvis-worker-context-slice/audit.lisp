(audit
  :id "20260524-jarvis-worker-context-slice"
  :schema "missiond.work-order.audit.v1"
  :status completed
  :summary "Jarvis worker prompts now carry the compact accepted execution slice needed by no-MCP external workers: target engine/pool, read/write scope, write policy, confirmed intent/plan artifact ids, acceptance, and context-pack path."
  :evidence
  ((check
     :command "cargo test -p missiond-core jarvis_worker_prompt_prefers_materialized_context_file -- --nocapture"
     :result pass)
   (check
     :command "node scripts/check-v3-behavior-closure.mjs --json"
     :result pass)
   (check
     :command "node scripts/check-v3-code-isomorphism-complete.mjs --json"
     :result pass)
   (check
     :command "node scripts/check-v3-final-convergence.mjs --json --static-only"
     :result pass)
   (check
     :command "git diff --check"
     :result pass))
  :changed
  ((file ".missiond/v3/shards/request-runtime.lisp"
     :reason "SSOT now requires Jarvis worker prompts to include confirmed intent/plan refs and accepted execution metadata.")
   (file "crates/missiond-core/src/ws/server.rs"
     :reason "Worker prompt builder now materializes engine/pool, scope, artifact refs, acceptance, and accepted execution slice.")
   (file "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
     :reason "Regenerated V3 contract projection.")
   (file "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
     :reason "Regenerated runtime defaults projection.")
   (file "scripts/generated/v3_contracts.d.ts"
     :reason "Regenerated JS/TS contract projection.")
   (file "scripts/generated/v3_contracts.mjs"
     :reason "Regenerated JS contract projection.")
   (file "scripts/generated/v3_runtime_defaults.mjs"
     :reason "Regenerated JS runtime defaults projection.")))
