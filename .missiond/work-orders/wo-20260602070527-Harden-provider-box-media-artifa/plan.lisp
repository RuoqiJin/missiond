(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260602070527-Harden-provider-box-media-artifa"
  :intent "wo-20260602070527-Harden-provider-box-media-artifa"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260602070527-Harden-provider-box-media-artifa-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/engine/"
                     "crates/missiond-daemon/src/handlers/compute/"
                     "crates/missiond-daemon/src/llm/gemini_driver.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/provider_box/"
                     "crates/missiond-daemon/src/slot_orchestrator/"
                     "crates/missiond-pty/src/"
                     "docs/guides/product-service-layer-template.md"
                     "scripts/check-project-ssot-universe.mjs"
                     "scripts/generated/"]
       :acceptance ["node scripts/check-v3-interactive-provider-box.mjs"
                    "node scripts/check-v3-runtime-domain-projections.mjs"
                    "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
                    "cargo test -p missiond-daemon provider_box::codex_driver -- --nocapture"
                    "cargo test -p missiond-daemon provider_box::http_adapter::tests::image_generation_response_exposes_media_artifact_and_image_url -- --nocapture"
                    "git diff --check"])))
