-- Phase 6: Drop deprecated tables per frozen lisp v0.4.23.
--
-- Authorized drops (lisp :status on each):
--   message_narrations  :: pending-drop-v0.4.12 (step-narrator worker 下架, 无 writer/reader)
--   narration_cursors   :: pending-drop-v0.4.12 (narration 防重游标, 同步 drop)
--   events              :: drop-candidate       (legacy task-event 路径, pillar 四 event_log 取代)
--   credentials         :: drop-candidate       (v0.4.15 调查确认 dead schema, 零 Rust CRUD)
--
-- NOT dropped this phase (lisp explicit "keep" / "deprecate-not-drop-yet"):
--   tasks   — lisp 标 "✓ KEEP — legacy slot API 仍在使用" (memory_scheduler / autopilot 活用)
--   inbox   — lisp 标 "⚠ DEPRECATE — 轻度使用" (autopilot + strategy_worker 仍写入)
-- → 下一轮清理 (v0.5 或 slot API 替换完成后) 再处理.
--
-- IF EXISTS 保证幂等 — 已在别处 drop 的表不会 error.

DROP TABLE IF EXISTS message_narrations CASCADE;
DROP TABLE IF EXISTS narration_cursors CASCADE;
DROP TABLE IF EXISTS events CASCADE;
DROP TABLE IF EXISTS credentials CASCADE;
