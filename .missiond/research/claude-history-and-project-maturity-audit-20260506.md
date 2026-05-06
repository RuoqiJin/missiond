# ClaudeCode history + project maturity audit, 2026-05-06

## Scope

This audit answers two questions from the active MissionD convergence thread:

1. Whether suspected duplicate ClaudeCode conversations/messages are real duplicate ingestion.
2. Whether projects that have an independent `*blueprint.lisp` actually satisfy `pillar -> function -> entry/core/egress/surface` with ordered core steps and code-isomorphism evidence.

## ClaudeCode duplicate audit

Database source: `conversations.source = 'claude_code'`.

Observed counts:

| Metric | Count |
| --- | ---: |
| ClaudeCode conversations | 5,675 |
| ClaudeCode messages | 625,481 |
| Messages with null `message_uuid` | 0 |
| Duplicate `message_uuid` groups | 0 |
| Fallback duplicate groups (`session_id + role + timestamp + content`) | 95 |
| Fallback extra rows | 115 |

Conclusion: ClaudeCode does not show the Codex-style structural duplicate-ingestion issue. The remaining fallback duplicates are small, content-level repeats. Representative samples are repeated provider/tool artifacts:

- `<tool_use_error>File does not exist.</tool_use_error>`
- `No such tool available: mcp__missiond__mission_kb_search`
- `No files found`
- `[tool_result]`
- rejected/cancelled tool call messages
- local command output such as `/exit`, `Goodbye!`, `Bye!`

Do not bulk-delete these yet. They are better handled by read-model coalescing and, if needed, a reviewed cleanup that preserves provider provenance.

## ClaudeCode classification audit

`mission_conversation_query(action=audit_classification, source=claude_code, minConfidence=0.9)` found high-confidence historical worker sessions and the repair was applied:

- Applied repairs: 16 conversations.
- Repair class: `claude_worker_prompt_signature`.
- Typical change: `subagent/user -> worker`.
- Turn rebuild was run for repaired sessions.
- Post-repair high-confidence classification candidates: 0.

A deeper role-attribution dry-run still reports historical message-role suspects:

| Audit | Result |
| --- | ---: |
| Scanned recent Claude sessions | 200 |
| Sessions with role suspects | 65 |
| `systemWorkerPromptSuspects` | 0 |
| `missingRawRoleRows` | 0 |
| `userInWorkerSessionSuspects` | 195 |

The samples are worker-slot sessions where provider `raw_role=user` local-command blocks remain stored as MissionD `role=user`, for example:

- `<local-command-caveat>...`
- `<command-name>/exit</command-name>`
- `<local-command-stdout>Goodbye!</local-command-stdout>`

Interpretation: conversation-level classification is fixed; a historical message-role backfill is still needed for worker sessions so local command / worker prompt rows display as `worker_user` or another non-human role. This should be a reviewed DB backfill, not a blind delete.

## Project maturity audit

The previous Universe registry was too optimistic. It marked several projects as M10 even when they only had an intent file or lacked explicit code-isomorphism/current-code-mapping evidence.

The checker now enforces:

- Intent-only projects cannot satisfy M3+.
- M6 requires a project-level `*blueprint.lisp`.
- M6 requires code-isomorphism/current-code-mapping evidence.
- M10 remains V3-parity, not just “has Lisp”.

Current honest maturity:

| Project | Current | Meaning |
| --- | --- | --- |
| missiond | M10 | V3-parity evidence present |
| board | M10 | frontend SSOT evidence present |
| jarvis-forge | M10 | blueprint + code-isomorphism evidence present |
| deploy-agent | M10 | blueprint + code-isomorphism evidence present |
| auth | M10 | blueprint + code-isomorphism evidence present |
| jarvis | M2 | intent/index only; needs blueprint split |
| jarvis-mechanic | M2 | intent/index only; needs blueprint split |
| xjpcode | M2 | intent/index only; needs blueprint split |
| neural-codegen | M2 | intent/index only; needs blueprint split |
| semantic-terminal | M2 | intent/index only; needs blueprint split |
| xiaojinpro-backend | M5 | blueprint exists, but lacks code-isomorphism/current-code-mapping evidence |
| deploy-center | M5 | blueprint exists, but lacks code-isomorphism/current-code-mapping evidence |
| router | M5 | blueprint exists, but lacks code-isomorphism/current-code-mapping evidence |
| payments | M5 | blueprint exists, but lacks code-isomorphism/current-code-mapping evidence |
| asr | M5 | blueprint exists, but lacks code-isomorphism/current-code-mapping evidence |
| timeline | M5 | blueprint exists, but lacks code-isomorphism/current-code-mapping evidence |
| pcea | M5 | blueprint exists, but lacks code-isomorphism/current-code-mapping evidence |
| secret-store | M5 | blueprint exists, but lacks code-isomorphism/current-code-mapping evidence |
| xiaojin-blog | M5 | blueprint exists, but lacks code-isomorphism/current-code-mapping evidence |
| cuthub | M5 | blueprint exists, but lacks code-isomorphism/current-code-mapping evidence |

This is intentionally conservative. A project with a real checker that the heuristic cannot detect should add explicit `code-isomorphism` / `current-code-mapping` / `implementation-map` evidence to its blueprint or registry checks.

## Gate results

Passing checks:

- `node scripts/check-project-ssot-universe.mjs --json`
- `node scripts/check-project-maturity.mjs --dry-fixture --json --min-level M6`
- `node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs`

Expected failing checks:

- `node scripts/check-project-maturity.mjs --json --min-level M6`
- `node scripts/check-project-maturity.mjs --json --min-level M10`
- `node scripts/check-v3-final-convergence.mjs --json --static-only`

Final convergence now fails honestly at `project-maturity` until the M2 and M5 projects are upgraded.

## Next infrastructure work

1. Add a reviewed ClaudeCode message-role backfill for worker sessions:
   - dry-run first;
   - update only `source='claude_code'`, `conversation_type='worker'`, `role='user'`, `raw_role='user'`;
   - target local-command and worker-prompt rows;
   - rebuild turns for touched sessions.
2. Use the stricter maturity gate as the dispatch source for project M10 work:
   - M2 projects first need blueprint split;
   - M5 projects need explicit code-isomorphism/current-code-mapping evidence and checker anchors.
3. Keep Universe honest: target remains M10 for all projects, but current maturity must reflect actual evidence.
