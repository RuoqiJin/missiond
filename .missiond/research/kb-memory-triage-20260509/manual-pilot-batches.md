# KB memory triage manual pilot — 2026-05-09

Policy calibrated before full workflow run. Scope: `knowledge.category = memory OR memory:%`.

Review state meaning used here:
- `active`: keep in default reasoning because it is a current preference, operating constraint, or external tool quirk not yet safely owned by SSOT.
- `superseded-by-lisp`: current SSOT/workflow/universe owns the fact; KB entry is no longer default context.
- `superseded-by-code`: implementation/checker owns the behavior; KB entry is only historical trace.
- `historical-evidence`: useful for archaeology, not active guidance.
- `needs-human`: potentially valuable but too broad/volatile; show in review, not default retrieval.
- `wrong-or-stale`: likely contradicted by current design or intentionally disabled.

## Batch 1: top 10 by access_count

| key | decision | rationale |
|---|---|---|
| strategic-state | needs-human | Too broad and stale for default context; contains useful preferences but must be split into project constants / user preference records before active use. |
| missiond-user-voice-extraction-pipeline | superseded-by-lisp | Current memory/conversation governance is in MissionD V3/workflows. |
| kb-cli-wrapper-for-non-mcp-ai | needs-human | May be useful as external access pattern, but current MCP/runtime status needs verification before active use. |
| assistant-service-progressive-disclosure | historical-evidence | XJP assistant design fact; should live in service SSOT if still current, not global memory. |
| slash-clear-is-valid-claude-code-command | active | Current provider behavior correction; prevents recurring false diagnosis. |
| kb-composite-category-design | superseded-by-code | Category behavior is schema/query implementation, not active memory. |
| kb-autonomous-consolidation-architecture | superseded-by-lisp | Entry already labels itself superseded; V3 memory workflow is canonical. |
| memory-extraction-meta-circulation-resolved | superseded-by-lisp | Entry already labels itself superseded. |
| missiond-gemini-call-sites-architecture | superseded-by-lisp | V3 workstation pool/router policy owns Gemini role/dispatch. |
| frontend-chat-dual-session | needs-human | Jarvis project fact; should be promoted to Jarvis SSOT or verified there before active memory. |

## Batch 2: rows 11-20

| key | decision | rationale |
|---|---|---|
| missiond-briefing-worker-minimax-architecture | superseded-by-lisp | Entry already labels itself superseded by workstation/model routing. |
| router-gemini-google-search-grounding | needs-human | Provider quirk may remain useful, but should be owned by router SSOT if current. |
| quark-api-technical-details | needs-human | Unregistered/external object-store fact; needs project owner before active use. |
| network-topology-overview | needs-human | Operational topology is volatile; should move to Universe/deploy-center constants after verification. |
| memory-slot-stuck-detection-evolution | historical-evidence | Historical debug evolution; current timeout/supervision must be code/workflow-owned. |
| baidu-netdisk-integration-architecture | needs-human | Feature/project ownership unclear; do not default-load. |
| missiond-runs-on-local-mac-not-privatecloud | superseded-by-lisp | Runtime location belongs to MissionD Universe/project registry. |
| router-billing-three-layer-system | needs-human | Potentially important router/payment fact; should be verified against router/payment SSOT. |
| missiond-ops-diagnostic-tools-implemented | superseded-by-lisp | Tool surfaces and registry own this. |
| router-four-connectors-architecture | needs-human | Router model/provider topology should be router SSOT; keep out of default until verified. |

## Batch 3: rows 21-30

| key | decision | rationale |
|---|---|---|
| board-ui-and-task-management-features | superseded-by-lisp | Entry already labels itself superseded by Board frontend SSOT. |
| gemini-vertex-json-schema-quirks | active | Current external provider quirk; useful until router/provider SSOT fully owns tested behavior. |
| private-cloud-dns-split-and-minimax-integration | historical-evidence | Old private-cloud + MiniMax ops history; likely not active in current worker pool. |
| deploy-agent-autoupdate-lifecycle | needs-human | Important deploy-agent fact but should be verified against deploy-agent/deploy-center SSOT. |
| missiond-context-budget-manager-architecture | superseded-by-lisp | Current context-budget/transport policy belongs to V3 conversation/workflow/runtime. |
| ios-jarvis-integration-architecture | needs-human | Jarvis/iOS fact; verify against Jarvis SSOT before active use. |
| missiond-maxsim-multi-topic-search-architecture | superseded-by-code | Search implementation owns this; not default reasoning memory. |
| jarvis-trace-store-ring-buffer | superseded-by-code | MissionD code/tool registry owns Jarvis trace surfaces. |
| missiond-subagent-parent-session-architecture | superseded-by-lisp | Entry already labels itself superseded. |
| jsonl-full-capture-dual-table-design | superseded-by-lisp | Entry already labels itself superseded by conversation ingestion. |

## Batch 4: rows 31-40

| key | decision | rationale |
|---|---|---|
| mcp-frontend-camelcase-contract-risk | active | Current interface casing risk; prevents frontend/API regressions. |
| verify-subagent-code-analysis-manually | active | Current user preference / operating rule. |
| missiond-embedding-provider-architecture | needs-human | Embedding plan changed several times; verify against V3/runtime before active use. |
| missiond-shared-http-client-for-router | superseded-by-code | Historical bugfix; implementation owns it. |
| auth-deploy-center-subdomain-allocation | superseded-by-lisp | Auth/deploy-center Universe owns domain facts. |
| pty-state-detection-v2-architecture | superseded-by-lisp | V3 PTY recognition/upstream signatures own current behavior. |
| mcp-only-deploy-docker-skip-normal | needs-human | Deploy behavior may still matter but should live in deploy-center SSOT. |
| claude-code-jsonl-role-mapping-quirks | active | Current provider log parsing quirk; relevant to ongoing role/turn audits. |
| agent-update-source-priority-2026-02-21 | needs-human | Deploy-agent source priority is operational and should be verified in deploy-agent SSOT. |
| missiond-token-usage-ledger-architecture | historical-evidence | SQLite-era design reason; current Postgres/event ledger should be code/SSOT-owned. |

## Batch 5: rows 41-50

| key | decision | rationale |
|---|---|---|
| missiond-claude-md-auto-sync | wrong-or-stale | Context preloading/autosync was intentionally disabled/reduced due KB noise; do not active-load. |
| jarvis-phase2-e2e-completed | historical-evidence | Completed milestone summary, not active guidance. |
| pty-screenshot-frontend-xterm-canvas | superseded-by-code | Current feature owned by code/frontend SSOT. |
| missiond-deep-analysis-trigger-architecture | superseded-by-lisp | Entry already labels itself superseded. |
| missiond-task-ack-mechanism | superseded-by-code | MCP tool/implementation owns this; not active memory. |
| missiond-slots-yaml-hot-reload-implemented | superseded-by-code | Runtime implementation owns this. |
| pty-logs-vs-direct-conversation-analysis-responsibility | active | Current user preference about what PTY logs are for. |
| timeline-ai-step-explanation-gpt-5.4 | needs-human | Product/design choice may still matter, but should be revalidated in timeline SSOT. |
| kb-write-summary-length-enforcement | active | Current KB quality rule; directly governs future memory writes. |
| missiond-confirm-screen-builtin-tool-parsing | superseded-by-code | Historical bugfix; code/tests own it. |

Calibration outcome: first 50 produce 7 active, 19 superseded, 8 historical/wrong, 16 needs-human. This is close to the desired ~10% active target while keeping uncertain facts visible for later review.
