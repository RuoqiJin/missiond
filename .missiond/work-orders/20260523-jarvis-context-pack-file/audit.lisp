(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "20260523-jarvis-context-pack-file"
  :status in-progress
  :notes ["Agy read-only smoke proved strict intent/plan gate works but also proved shared-artifact-only context is insufficient for provider CLIs without MissionD MCP."
          "Fix materializes a bounded context_pack_file under .missiond/v3/runtime/context-gather and carries it through Jarvis SSE, intent, plan, BoardTask metadata, and worker prompt."
          "Provider prompt now prefers context_pack_file and only uses mission_shared_memory / mission_context_slice when the file is unavailable."])
