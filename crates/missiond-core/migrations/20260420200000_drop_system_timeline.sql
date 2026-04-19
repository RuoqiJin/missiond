-- Event Bus v2 · SSOT cutover · drop legacy system_timeline
--
-- v1.3.0 §4.6 :ssot-declaration "event_log = timeline SSOT".
--   - event_log is the only truth source for timeline data.
--   - All readers (mission_timeline MCP, WS /events catch-up, timeline_analyst)
--     now use `missiond_core::event::projection` which queries event_log.
--   - The v1 timeline-writer subscriber was removed in Phase 8 of the
--     event-bus migration (see intent-event-bus-execution.lisp D014).
--
-- Historical data in system_timeline is not migrated: the lisp's
-- (historical-data-policy) declared it read-only for 90 days and then dropped;
-- that window has elapsed.

DROP TABLE IF EXISTS system_timeline CASCADE;
-- CASCADE also removes the GIN/GIN-trgm indexes declared in
-- 20260318000000_init.sql (idx_tl_*) that reference the table.
