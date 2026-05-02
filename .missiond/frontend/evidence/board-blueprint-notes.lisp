(frontend-blueprint-evidence
  :schema "missiond.frontend-blueprint-evidence.v1"
  :project board
  :blueprint ".missiond/frontend/board-blueprint.lisp"
  (note frontend-ssot-scope
    :text "Board frontend keeps a project-local Lisp SSOT so the same pattern can be reused by later MissionD-managed projects. Backend V3 owns the registry pointer and aggregate gates; frontend behavior lives here.")
  (note runtime-slot-projection
    :text "Static SLOT_OPTIONS were an old UI convenience. Runtime workstation identity must project from mission_slots + mission_pty_status so ClaudeCode, Gemini, and Codex pool changes are visible without frontend code edits.")
  (note first-wave-policy
    :text "First wave preserves existing UI layout and behavior. Refactors are limited to Lisp contract, checkers, runtime projection, and provider-neutral copy."))
