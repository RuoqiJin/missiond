-- v0.5.0: memory-hook 迁 board_tasks 准备
-- board_tasks 加 trigger_source 列, 支持按触发源 (memory_hook / user / autopilot / flow / cli) 区分
--
-- 不 DROP TABLE tasks — pillar 二 (mission_task_submit MCP 族 / supervisor / pty / context_pipeline / extraction) 仍在用.
-- 本 migration 仅为 memory-hook 5 caller 迁移做准备.

ALTER TABLE board_tasks ADD COLUMN IF NOT EXISTS trigger_source TEXT;

CREATE INDEX IF NOT EXISTS idx_board_tasks_trigger_source
    ON board_tasks(trigger_source)
    WHERE trigger_source IS NOT NULL;
