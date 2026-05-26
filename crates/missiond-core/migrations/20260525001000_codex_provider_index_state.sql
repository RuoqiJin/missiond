-- Rename Codex provider-local source-state labels so MissionD-owned storage
-- uses provider-local terminology. Codex's external local index may still be
-- read through the provider adapter.
UPDATE conversation_source_state
SET raw_state = 'provider-index-missing',
    reason = replace(
        COALESCE(reason, ''),
        'Codex state_5.sqlite has no matching thread row',
        'Codex provider-local thread index has no matching thread row'
    )
WHERE source = 'codex_cli'
  AND raw_state = 'sqlite-missing';
