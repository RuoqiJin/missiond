# ClaudeCode Role Attribution Dry-Run Report - 2026-05-03

Command:

```bash
node scripts/report-claude-role-attribution.mjs --json --session-limit 20 --tail 200
```

Result summary:

| Metric | Value |
| --- | ---: |
| scanned sessions | 14 |
| sessions with suspects | 13 |
| system worker-prompt suspects | 1 |
| user-in-worker-session suspects | 3 |
| missing rawRole rows | 516 |

Interpretation:

- The current root issue is mostly historical rows without `rawRole`, especially
  `thinking` and `tool_result` rows.
- Only one recent scanned session showed a worker prompt-like message stored as
  `system`, and three rows showed `user` inside a worker session. These are
  old-data/backfill candidates, not a reason to mutate DB immediately.
- New ingestion paths now normalize automated slot `raw_role=user` into
  `worker_user`, preserve `rawRole`, and keep interactive/Jarvis input as
  `user`.

Next action:

1. Deploy the new role normalizer.
2. Re-run the dry-run audit after new worker sessions appear.
3. Design a reviewed DB backfill only if historical rows need correction for
   product UX or memory distillation. Do not bulk mutate old rows from this
   report alone.
