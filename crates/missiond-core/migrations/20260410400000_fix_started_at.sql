-- Fix conversations where started_at was set to DB insertion time instead of actual start.
-- Symptom: started_at > updated_at (impossible in real usage).
-- Fix: set started_at to the earliest message timestamp for that conversation.

UPDATE conversations c
SET started_at = sub.first_ts
FROM (
    SELECT session_id, MIN(timestamp) AS first_ts
    FROM conversation_messages
    GROUP BY session_id
) sub
WHERE c.id = sub.session_id
  AND c.started_at > c.updated_at
  AND sub.first_ts IS NOT NULL;
