;; MissionD v1 legacy Lisp manifest.
;; This file organizes the original root-level intent*.lisp corpus without
;; moving compatibility paths that current tooling may still scan directly.

(missiond-v1-lisp-manifest
  :schema "missiond.v1.manifest.v0"
  :version "v1-legacy-organized"
  :status "reference-index"
  :compatibility "Root .missiond/intent*.lisp files remain compatibility paths; this manifest is the organized v1 entry point."
  :successor ".missiond/v3/missiond-blueprint.lisp"

  (principles
    (keep-root-compatibility
      :reason "mission_intent path discovery still scans root .missiond intent files")
    (index-before-move
      :reason "Do not move legacy files until code and checker resolvers understand .missiond/v1")
    (historical-not-runtime
      :reason "v1 explains early architecture; v3 becomes the compact runtime contract"))

  (group kernel
    :purpose "Top-level map, shared types, pure helpers, and early flow vocabulary."
    (file :id intent-root             :path ".missiond/intent.lisp")
    (file :id intent-types            :path ".missiond/intent-types.lisp")
    (file :id intent-pure-utility     :path ".missiond/intent-pure-utility.lisp")
    (file :id intent-flows            :path ".missiond/intent-flows.lisp"))

  (group db
    :purpose "Early database architecture and table-family splits."
    (file :id intent-db               :path ".missiond/intent-db.lisp")
    (file :id intent-db-audit         :path ".missiond/intent-db-audit.lisp")
    (file :id intent-db-compute       :path ".missiond/intent-db-compute.lisp")
    (file :id intent-db-conv          :path ".missiond/intent-db-conv.lisp")
    (file :id intent-db-kb            :path ".missiond/intent-db-kb.lisp")
    (file :id intent-db-misc          :path ".missiond/intent-db-misc.lisp")
    (file :id intent-db-pipeline      :path ".missiond/intent-db-pipeline.lisp")
    (file :id intent-db-skill         :path ".missiond/intent-db-skill.lisp")
    (file :id pillar-db-core          :path ".missiond/intent-pillar-db-core.lisp")
    (file :id pillar-db-agents        :path ".missiond/intent-pillar-db-agents.lisp")
    (file :id pillar-db-observability :path ".missiond/intent-pillar-db-observability.lisp")
    (file :id pillar-db-pipeline      :path ".missiond/intent-pillar-db-pipeline.lisp"))

  (group tools-and-transport
    :purpose "Early MCP/RPC/tool dispatch surfaces."
    (file :id intent-mcp-defs                 :path ".missiond/intent-mcp-defs.lisp")
    (file :id intent-rpc-gateway              :path ".missiond/intent-rpc-gateway.lisp")
    (file :id pillar-mcp-dispatch             :path ".missiond/intent-pillar-mcp-dispatch.lisp")
    (file :id pillar-transport-bootstrap      :path ".missiond/intent-pillar-transport-bootstrap.lisp"))

  (group engines-and-workers
    :purpose "Early engine, worker, LLM-context, parser, and state-machine pillars."
    (file :id intent-domain-engines        :path ".missiond/intent-domain-engines.lisp")
    (file :id pillar-engines               :path ".missiond/intent-pillar-engines.lisp")
    (file :id pillar-event-workers         :path ".missiond/intent-pillar-event-workers.lisp")
    (file :id pillar-llm-context           :path ".missiond/intent-pillar-llm-context.lisp")
    (file :id pillar-semantic-parser       :path ".missiond/intent-pillar-semantic-parser.lisp")
    (file :id pillar-standalone            :path ".missiond/intent-pillar-standalone.lisp")
    (file :id pillar-state-machines        :path ".missiond/intent-pillar-state-machines.lisp"))

  (translations
    :purpose "Human-readable zh mirrors and old zh snapshots; not part of runtime contract."
    (pattern ".missiond/intent*.lisp.zh")
    (pattern ".missiond/intent*.lisp.zh.old")
    (pattern ".missiond/intent*.lisp.zh.v1"))

  (migration
    (v1-to-v2
      :meaning "v2 expands v1 into pillar shards, source-index anchors, and implementation status history."
      :entry ".missiond/v2/intent.lisp"
      :source-index ".missiond/v2/intent-pillar-source-index.lisp")
    (v2-to-v3
      :meaning "v3 compresses the core into executable request/artifact/state-machine/policy contracts."
      :entry ".missiond/v3/missiond-blueprint.lisp"
      :rule "History stays in v2/source-index; runtime truth moves to v3 contracts.")))
