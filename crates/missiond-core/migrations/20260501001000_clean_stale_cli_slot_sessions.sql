-- Remove historical slot->conversation bindings when the slot's provider and
-- the stored conversation source disagree. Runtime can then rebind the slot to
-- the next real provider session instead of surfacing stale state.

DELETE FROM slot_sessions ss
USING conversations c
WHERE c.id = ss.session_id
  AND (
    (ss.slot_id ILIKE '%gemini%' AND c.source <> 'gemini_cli')
    OR (ss.slot_id ILIKE '%codex%' AND c.source <> 'codex_cli')
    OR (
      (
        ss.slot_id ILIKE '%claude%'
        OR ss.slot_id IN ('foreground', 'orchestrator', 'lisp-surveyor', 'slot-arch-maint')
      )
      AND c.source <> 'claude_code'
    )
  );
