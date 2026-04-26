;; Wave 19 shared-memory ledger.
;; Schema: .missiond/tasks/schema/shared-memory-v1.lisp
;; Checker: scripts/check-task-memory.mjs
;;
;; Append-only. Agents add entries while they hold a live :claim for their
;; task id. Editing or removing prior entries is forbidden — corrections are
;; recorded as new (correction ...) entries that :refs the original :id.

(shared-memory wave19
  :schema "missiond.shared-memory.v1"
  :wave wave19
  :created-at "2026-04-26T00:00:00Z"
  :sequence 1

  (observation
    :id wave19-04-bootstrap-001
    :task wave19-04-shared-memory-ledger-v0
    :agent claudecode
    :seq 1
    :at "2026-04-26T00:00:00Z"
    :touched []
    :summary "Bootstrap entry: ledger header established. Future entries appended below by parallel wave19 agents."))
