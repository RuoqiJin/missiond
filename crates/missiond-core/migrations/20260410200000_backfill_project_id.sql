-- Backfill project_id for historical conversations.
--
-- The previous revision of this migration hard-coded an operator-specific
-- list of project paths. That has been removed so the public build is
-- environment-agnostic; projects are now registered at runtime via
-- `mission_project init` and `ProjectRegistry`, which resolves cwd →
-- project_id via longest-prefix matching.
--
-- This file is left in place so the migration ordering stays stable.
SELECT 1;
