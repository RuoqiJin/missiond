(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260603031200-deploy-active-commit-guard"
  :events ((event created :at "2026-06-02T19:12:24.925Z" :actor missiond-work-order)
           (event active-commit-guard
             :at "2026-06-02T19:16:58Z"
             :actor codex
             :summary "deploy-daemon now rejects same-owner stale commit releases unless MISSIOND_DEPLOY_ALLOW_COMMIT_REGRESSION=1 is explicit; release manifest and closure evidence carry git_full_sha/active_git_sha provenance.")))
