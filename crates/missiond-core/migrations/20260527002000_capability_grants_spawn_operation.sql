-- Keep capability_grants.operation aligned with the control-plane runtime.
--
-- 20260527000000 created the table before spawn capability was enforced by
-- mission_compute_slot and task delegation. Existing databases need the check
-- constraint widened so read-only worker artifact materialization can grant the
-- task-scoped spawn capability required by the runtime gate.

ALTER TABLE capability_grants
  DROP CONSTRAINT IF EXISTS capability_grants_operation_check;

ALTER TABLE capability_grants
  ADD CONSTRAINT capability_grants_operation_check
  CHECK (operation IN ('read', 'write', 'claim', 'settle', 'delegate', 'deploy', 'network', 'spawn'));
