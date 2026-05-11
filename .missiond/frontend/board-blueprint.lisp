(missiond-frontend-blueprint
  :schema "missiond.frontend-blueprint.v1"
  :project board
  :app "@missiond/board"
  :root "packages/board"
  :authority "project-local-lisp-ssot"
  :parent-v3 ".missiond/v3/missiond-blueprint.lisp"
  :status "code-aligned"
  :maturity M10
  :evidence-root ".missiond/frontend/evidence"
  :context-pack-root ".missiond/frontend/context-packs"

  (project
    :id board
    :kind nextjs-dashboard
    :package "packages/board/package.json"
    :runtime [nextjs react zustand xterm missiond-ipc eventbus-ws]
    :non-goals [visual-redesign backend-contract-rewrite])

  (runtime-projection
    :rule "Frontend runtime choices project from MissionD tools/events or this Lisp; do not reintroduce stale static workstation pools."
    (projection workstation-slots
      :source [mission_slots mission_pty_status workstation-pool]
      :entry ["packages/board/src/app/api/slots/route.ts"]
      :consumers ["packages/board/src/App.tsx"
                  "packages/board/src/components/Terminal.tsx"
                  "packages/board/src/components/TaskDialog.tsx"
                  "packages/board/src/components/AutopilotMonitor.tsx"
                  "packages/board/src/components/ExecDashboard.tsx"]
      :fields [id label role running state provider engine modelProfile reasoningEffort searchEnabled sandbox approvalPolicy taskClass acceptsBoardTask mcpReady mcpEnabled mcpApprovalReady mcpApprovalMissingTools mcpSource providerConversationId confidence reason activeTool blockedKind currentTaskId activeBoardTaskId latestConversation]
      :state-contract
        ((running-states [running thinking responding tool_running confirming blocked starting])
         (inactive-states [complete completed exited not_running stopped dead missing error])
         (rule "Normalize PTY state case before classification; never trust stale raw status.running for terminal states.")
         (rule "All V3 projected pool providers must be visible: claude-code-default, claude-code-fast-patch, gemini-ultra-pro, gemini-fast-survey, and codex-master-control.")
         (rule "Codex master-control rows must surface missiond MCP readiness and MCP approval readiness from mission_master_status/mission_slots so the operator can distinguish a stopped PTY from a missing control-plane tool or unattended approval blocker.")
         (rule "Codex rows must prefer providerConversationId/latest real codex_cli conversation over PTY placeholder conversations so durable logs are visible in the cockpit.")
         (rule "Codex master and Gemini PTYs must be selectable exactly like ClaudeCode PTYs; provider-specific copy lives in labels, not in visibility filters.")
         (rule "A stale placeholder conversation or future timestamp must be marked diagnostic/mismatch and cannot make a stopped PTY appear running.")
         (rule "BoardTask-to-slot visibility must prefer runtime-projected activeBoardTaskId/currentTaskId, then BoardTask claimExecutorId/assignee."))
      :forbid [SLOT_OPTIONS hardcoded-sonnet-label stale-status-running])
    (projection pty-recognition
      :source [mission_pty_status provider-aware-recognition]
      :entry ["packages/board/src/components/Terminal.tsx"]
      :fields [state statusText blockedKind provider confidence]
      :rule "Terminal labels must describe the selected provider/session generically; no Claude-only copy in shared PTY surfaces.")
    (projection terminal-slot-selector
      :source [mission_slots mission_pty_status localStorage]
      :entry ["packages/board/src/App.tsx"
              "packages/board/src/components/Terminal.tsx"]
      :fields [id label role running state provider taskClass acceptsBoardTask activeSlot]
      :rule "Terminal slot selection lives inside the Terminal cockpit, lists all projected slots by provider group, keeps labels bounded/truncated, and pairs the PTY with durable conversation/evidence diagnostics.")
    (projection eventbus-cache-invalidation
      :source [eventbus-ws]
      :entry ["packages/board/src/eventStream.ts"]
      :fields [seq type payload version-counter custom-event])
    (projection board-task-contract
      :source [mission_board mission_task_delegate]
      :entry ["packages/board/src/api.ts"
              "packages/board/src/store.ts"
              "packages/board/src/app/api/tasks/route.ts"]
      :fields [status priority category assignee autoExecute promptTemplate flowTemplate dependsOn leaseExpiresAt notes])
    (projection next-api-runtime-boundary
      :source [nextjs-route-handler missiond-ipc]
      :entry ["packages/board/src/app/api/master/status/route.ts"]
      :fields [runtime dynamic revalidate]
      :rule "Runtime-only MissionD proxy API routes must export nodejs runtime, force-dynamic, and revalidate=0 so production builds never collect MissionD IPC data at build time.")
    (projection service-runtime-universe
      :source [mission_project.universe service-runtime-universe]
      :entry ["packages/board/src/app/api/projects/route.ts"
              "packages/board/src/components/SystemDashboard.tsx"]
      :fields [serviceId project root environment publicBaseUrl issuer domains dnsProvider dnsCapability deployment proxy ports health dependencies opsCapability sourceEvidence risks]
      :rule "SystemDashboard must show production service domain/deployment/DNS capability from MissionD Universe, not from local hardcoded cards.")
    (projection infrastructure-universe
      :source [mission_infra_query.infrastructure-universe mission_infra_query.skill_evidence mission_infra_query.credential_refs mission_infra_query.reconcile]
      :entry ["packages/board/src/app/api/infra/route.ts"
              "packages/board/src/components/SystemDashboard.tsx"]
      :fields [runtimeTargets credentialRefs skillEvidence reconcileDrift freshness credentialInlineRisk]
      :rule "SystemDashboard must show runtime targets, skill-derived evidence, secret refs, and reconcile drift as read-only governance data; it must never display credential values.")
    (projection decision-inbox
      :source [mission_question DecisionDashboard JarvisChat]
      :entry ["packages/board/src/components/DecisionDashboard.tsx"
              "packages/board/src/components/SystemDashboard.tsx"
              "packages/board/src/app/api/questions/route.ts"]
      :fields [id question context status target options decisionType answer taskId createdAt updatedAt revalidationStatus staleReason evidenceFreshAt]
      :rule "User-facing decisions live as durable mission_question records; frontend Decision Inbox is canonical, while master/Jarvis messages are reminders and explanation channels. Operational decisions must be revalidated by MissionD before display; stale/resolved runtime evidence should be shown as answered diagnostics or removed from pending, not presented as a fresh user choice."))

  (frontend-runtime-config
    :schema "missiond.frontend-runtime-config.v1"
    :generator "node scripts/project-frontend-board-config.mjs --write"
    :checker "node scripts/project-frontend-board-config.mjs --check"
    :output "packages/board/src/generated/board-frontend-config.ts"
    :rule "Frontend enums, tabs, and EventBus route tables project from Lisp into generated TypeScript; hand-edited runtime config is drift."
    (tabs
      :default jarvis
      (tab :id jarvis :label "Jarvis" :icon Sparkles)
      (tab :id board :label "Board" :icon ClipboardList)
      (tab :id terminal :label "Terminal" :icon MonitorUp)
      (tab :id exec :label "Exec" :icon Crosshair)
      (tab :id system :label "System" :icon Gauge)
      (tab :id knowledge :label "Knowledge" :icon Brain)
      (tab :id logs :label "Logs" :icon MessageSquareText)
      (migration :from autopilot :to exec)
      (migration :from decisions :to exec)
      (migration :from memory :to system)
      (migration :from engine :to system)
      (migration :from conversations :to logs)
      (migration :from timeline :to logs)
      (migration :from architecture :to knowledge)
      (migration :from deploy :to board)
      (migration :from research :to board))
    (task-taxonomy
      (category :id deploy :label "部署" :className "bg-orange-500/10 text-orange-400 border-orange-500/20")
      (category :id dev :label "开发" :className "bg-blue-500/10 text-blue-400 border-blue-500/20")
      (category :id infra :label "基建" :className "bg-purple-500/10 text-purple-400 border-purple-500/20")
      (category :id test :label "测试" :className "bg-green-500/10 text-green-400 border-green-500/20")
      (category :id research :label "研究" :className "bg-cyan-500/10 text-cyan-400 border-cyan-500/20")
      (category :id diagnosis :label "诊断" :className "bg-rose-500/10 text-rose-400 border-rose-500/20")
      (category :id investigation :label "调查" :className "bg-amber-500/10 text-amber-400 border-amber-500/20")
      (category :id other :label "其他" :className "bg-neutral-500/10 text-neutral-400 border-neutral-500/20")
      (priority :id high :label "高" :dotColor "bg-red-500")
      (priority :id medium :label "中" :dotColor "bg-yellow-500")
      (priority :id low :label "低" :dotColor "bg-blue-500")
      (group-option :value none :label "不分组")
      (group-option :value category :label "按分类")
      (group-option :value priority :label "按优先级")
      (group-option :value project :label "按项目")
      (server-option :value "私有云")
      (server-option :value "ECS")
      (server-option :value "GCP")
      (server-option :value "Win Agent"))
    (board-task-api-contract
      :route "/api/tasks"
      :frontend-fields [id title description status priority category project server dueDate parentId hidden createdAt updatedAt order claimExecutorId claimExecutorType claimedAt assignee autoExecute promptTemplate flowTemplate flowPhase dependsOn leaseExpiresAt timeoutSecs contextIntent notesCount notes]
      :backend-fields [id title description status priority category project server dueDate parentId hidden createdAt updatedAt orderIdx claimExecutorId claimExecutorType claimedAt assignee autoExecute promptTemplate flowTemplate flowPhase dependsOn leaseExpiresAt timeoutSecs contextIntent notesCount notes]
      (field-map :frontend order :backend orderIdx :default 0)
      (defaults :status open :priority medium :category other :description "")
      (action :name list :method GET :tool mission_board_list)
      (action :name get :method GET :tool mission_board_get)
      (action :name create :method POST :tool mission_board_create)
      (action :name update :method PATCH :tool mission_board_update)
      (action :name delete :method DELETE :tool mission_board_delete)
      (action :name toggle :method POST :tool mission_board_toggle)
      (action :name note-add :method POST :tool mission_board_note_add)
      (action :name clear-done :method POST :tool mission_board_delete :mode client-subtree-filter))
    (flow
      (template :value "" :label "无（普通任务）")
      (template :value engineering :label "Engineering Flow")
      (phase :id investigate :label "调查")
      (phase :id consult_gemini_1 :label "咨询1")
      (phase :id plan :label "方案")
      (phase :id consult_gemini_2 :label "咨询2")
      (phase :id execute :label "执行")
      (phase :id finalize :label "收尾")
      (phase :id done :label "完成"))
    (event-routes
      :resync-bumps [slotVersion taskVersion questionVersion decisionVersion memoryVersion deployVersion engineVersion timelineVersion]
      (route :events [health_snapshot] :bump [engineVersion] :health-snapshot true)
      (route :events [slot_state_changed slot_task_dispatched] :bump [slotVersion timelineVersion])
      (route :events [task_lifecycle] :bump [taskVersion timelineVersion])
      (route :events [question_created question_resolved] :bump [questionVersion timelineVersion])
      (route :events [decision_made] :bump [decisionVersion timelineVersion])
      (route :events [memory_phase_changed] :bump [memoryVersion timelineVersion])
      (route :events [cli_request_started cli_request_completed cli_tool_activity gemini_request_started gemini_request_completed gemini_tool_activity codex_request_started codex_request_completed] :bump [engineVersion timelineVersion])
      (route :events [board_task_updated] :bump [taskVersion timelineVersion] :deploy-category-bump true)
      (route :events [insight_generated briefing_batch_started git_commit board_task_created board_task_status_changed board_task_note_added board_task_claimed board_task_deleted translation_started translation_completed translation_failed] :bump [timelineVersion])
      (route :events [user_message assistant_message thinking_message system_message] :bump [timelineVersion] :delay-ms 500)
      (custom-event :event briefing_summary_generated :name "timeline-summary-update" :detail [target_seq summary])
      (custom-event :event jarvis_task_completed :name "jarvis-task-completed" :detail [conversation_id task_id])
      (prefix-route :prefix "narration_" :bump [timelineVersion] :delay-ms 500))
    (timeline-visuals
      :rule "Timeline event, slot, lane, session, and window visual taxonomy is Lisp data; TSX only maps icon names to components."
      (event :type user_message :dot "bg-blue-400" :glow "shadow-blue-400/50" :bg "bg-blue-400/10" :text "text-blue-400" :label "User" :icon MessageSquare)
      (event :type assistant_message :dot "bg-teal-400" :glow "shadow-teal-400/50" :bg "bg-teal-400/10" :text "text-teal-400" :label "Assistant" :icon Brain)
      (event :type thinking_message :dot "bg-violet-400" :glow "shadow-violet-400/50" :bg "bg-violet-400/10" :text "text-violet-400" :label "Thinking" :icon Brain)
      (event :type cli_request_started :dot "bg-fuchsia-400" :glow "shadow-fuchsia-400/50" :bg "bg-fuchsia-400/10" :text "text-fuchsia-400" :label "CLI ▸" :icon Cpu)
      (event :type cli_request_completed :dot "bg-fuchsia-500" :glow "shadow-fuchsia-500/50" :bg "bg-fuchsia-500/10" :text "text-fuchsia-400" :label "CLI ◂" :icon Cpu)
      (event :type cli_tool_activity :dot "bg-fuchsia-300" :glow "shadow-fuchsia-300/50" :bg "bg-fuchsia-300/10" :text "text-fuchsia-300" :label "CLI Tool" :icon Wrench)
      (event :type gemini_request_started :dot "bg-fuchsia-400" :glow "shadow-fuchsia-400/50" :bg "bg-fuchsia-400/10" :text "text-fuchsia-400" :label "Gemini ▸" :icon Cpu)
      (event :type gemini_request_completed :dot "bg-fuchsia-500" :glow "shadow-fuchsia-500/50" :bg "bg-fuchsia-500/10" :text "text-fuchsia-400" :label "Gemini ◂" :icon Cpu)
      (event :type codex_request_started :dot "bg-lime-400" :glow "shadow-lime-400/50" :bg "bg-lime-400/10" :text "text-lime-400" :label "GPT ▸" :icon Zap)
      (event :type codex_request_completed :dot "bg-lime-500" :glow "shadow-lime-500/50" :bg "bg-lime-500/10" :text "text-lime-400" :label "GPT ◂" :icon Zap)
      (event :type git_commit :dot "bg-yellow-400" :glow "shadow-yellow-400/50" :bg "bg-yellow-400/10" :text "text-yellow-400" :label "Commit" :icon GitCommit)
      (event :type task_lifecycle :dot "bg-sky-400" :glow "shadow-sky-400/50" :bg "bg-sky-400/10" :text "text-sky-400" :label "Task" :icon Activity)
      (event :type question_created :dot "bg-amber-400" :glow "shadow-amber-400/50" :bg "bg-amber-400/10" :text "text-amber-400" :label "Question" :icon MessageSquare)
      (event :type question_resolved :dot "bg-amber-300" :glow "shadow-amber-300/50" :bg "bg-amber-300/10" :text "text-amber-300" :label "Resolved" :icon MessageSquare)
      (event :type decision_made :dot "bg-orange-400" :glow "shadow-orange-400/50" :bg "bg-orange-400/10" :text "text-orange-400" :label "Decision" :icon GitBranch)
      (event :type insight_generated :dot "bg-emerald-400" :glow "shadow-emerald-400/50" :bg "bg-emerald-400/10" :text "text-emerald-400" :label "Insight" :icon Sparkles)
      (event :type board_task_created :dot "bg-indigo-400" :glow "shadow-indigo-400/50" :bg "bg-indigo-400/10" :text "text-indigo-400" :label "Created" :icon Activity)
      (event :type board_task_status_changed :dot "bg-indigo-300" :glow "shadow-indigo-300/50" :bg "bg-indigo-300/10" :text "text-indigo-300" :label "Status" :icon Activity)
      (event :type board_task_note_added :dot "bg-indigo-200" :glow "shadow-indigo-200/50" :bg "bg-indigo-200/10" :text "text-indigo-200" :label "Note" :icon MessageSquare)
      (event :type board_task_claimed :dot "bg-indigo-500" :glow "shadow-indigo-500/50" :bg "bg-indigo-500/10" :text "text-indigo-400" :label "Claimed" :icon Wrench)
      (event :type board_task_deleted :dot "bg-red-400" :glow "shadow-red-400/50" :bg "bg-red-400/10" :text "text-red-400" :label "Deleted" :icon AlertTriangle)
      (event :type board_task_updated :dot "bg-indigo-300" :glow "shadow-indigo-300/50" :bg "bg-indigo-300/10" :text "text-indigo-300" :label "Board" :icon Activity)
      (event :type slot_state_changed :dot "bg-slate-400" :glow "shadow-slate-400/50" :bg "bg-slate-400/10" :text "text-slate-400" :label "Slot" :icon Settings2)
      (event :type memory_phase_changed :dot "bg-cyan-400" :glow "shadow-cyan-400/50" :bg "bg-cyan-400/10" :text "text-cyan-400" :label "Memory" :icon Brain)
      (event :type briefing_batch_started :dot "bg-rose-300" :glow "shadow-rose-300/50" :bg "bg-rose-300/10" :text "text-rose-300" :label "Briefing" :icon Sparkles)
      (event :type briefing_summary_generated :dot "bg-rose-400" :glow "shadow-rose-400/50" :bg "bg-rose-400/10" :text "text-rose-400" :label "Summary" :icon Sparkles)
      (event :type system_message :dot "bg-slate-400" :glow "shadow-slate-400/50" :bg "bg-slate-400/10" :text "text-slate-400" :label "Daemon" :icon Terminal)
      (event :type slot_task_dispatched :dot "bg-amber-400" :glow "shadow-amber-400/50" :bg "bg-amber-400/10" :text "text-amber-400" :label "Dispatch" :icon ArrowRight)
      (event :type translation_started :dot "bg-indigo-400" :glow "shadow-indigo-400/50" :bg "bg-indigo-400/10" :text "text-indigo-400" :label "Translating" :icon Languages)
      (event :type translation_completed :dot "bg-indigo-500" :glow "shadow-indigo-500/50" :bg "bg-indigo-500/10" :text "text-indigo-400" :label "Translated" :icon CheckCheck)
      (event :type translation_failed :dot "bg-red-400" :glow "shadow-red-400/50" :bg "bg-red-400/10" :text "text-red-400" :label "Trans Err" :icon AlertTriangle)
      (event :type narration_session_started :dot "bg-violet-300" :glow "shadow-violet-300/50" :bg "bg-violet-300/10" :text "text-violet-300" :label "Narrating" :icon Sparkles)
      (event :type narration_batch_completed :dot "bg-violet-400" :glow "shadow-violet-400/50" :bg "bg-violet-400/10" :text "text-violet-400" :label "Batch Done" :icon CheckCheck)
      (event :type narration_session_completed :dot "bg-violet-500" :glow "shadow-violet-500/50" :bg "bg-violet-500/10" :text "text-violet-400" :label "Narrated" :icon CheckCheck)
      (event :type narration_failed :dot "bg-red-400" :glow "shadow-red-400/50" :bg "bg-red-400/10" :text "text-red-400" :label "Narr Err" :icon AlertTriangle)
      (slot-color :id slot-coder-1 :badge "bg-blue-500/20 text-blue-300" :border "border-l-blue-400" :line "rgba(96,165,250,0.25)")
      (slot-color :id slot-coder-bypass :badge "bg-indigo-500/20 text-indigo-300" :border "border-l-indigo-400" :line "rgba(129,140,248,0.25)")
      (slot-color :id slot-deploy-1 :badge "bg-orange-500/20 text-orange-300" :border "border-l-orange-400" :line "rgba(251,146,60,0.25)")
      (slot-color :id slot-memory :badge "bg-cyan-500/20 text-cyan-300" :border "border-l-cyan-400" :line "rgba(34,211,238,0.25)")
      (slot-color :id slot-memory-slow :badge "bg-teal-500/20 text-teal-300" :border "border-l-teal-400" :line "rgba(45,212,191,0.25)")
      (slot-fallback-line :value "rgba(168,85,247,0.25)")
      (slot-fallback-line :value "rgba(244,114,182,0.25)")
      (slot-fallback-line :value "rgba(74,222,128,0.25)")
      (slot-fallback-line :value "rgba(251,191,36,0.25)")
      (swimlane :id chat :label "Chat" :dot "bg-indigo-400" :css "#818cf8" :bg "bg-indigo-500/[0.03]" :types [user_message assistant_message thinking_message])
      (swimlane :id slot :label "Slot" :dot "bg-teal-400" :css "#2dd4bf" :bg "bg-teal-500/[0.03]" :types [slot_task_dispatched system_message])
      (swimlane :id ai :label "AI / LLM" :dot "bg-emerald-400" :css "#34d399" :bg "bg-emerald-500/[0.03]" :types [cli_request_started cli_request_completed cli_tool_activity gemini_request_started gemini_request_completed decision_made insight_generated])
      (swimlane :id gpt :label "GPT" :dot "bg-amber-400" :css "#fbbf24" :bg "bg-amber-500/[0.03]" :types [codex_request_started codex_request_completed])
      (swimlane :id code :label "Code" :dot "bg-blue-400" :css "#60a5fa" :bg "bg-blue-500/[0.03]" :types [git_commit])
      (swimlane :id flow :label "Flow" :dot "bg-orange-400" :css "#fb923c" :bg "bg-orange-500/[0.03]" :types [task_lifecycle question_created question_resolved])
      (swimlane :id board :label "Board" :dot "bg-pink-400" :css "#f472b6" :bg "bg-pink-500/[0.03]" :types [board_task_created board_task_status_changed board_task_note_added board_task_claimed board_task_deleted board_task_updated])
      (swimlane :id sys :label "System" :dot "bg-slate-400" :css "#94a3b8" :bg "bg-slate-500/[0.03]" :types [slot_state_changed memory_phase_changed briefing_batch_started briefing_summary_generated narration_session_started narration_batch_completed narration_session_completed narration_failed])
      (session-color :dot "bg-cyan-400" :line "rgba(34,211,238,0.25)" :ring "ring-cyan-400/40")
      (session-color :dot "bg-green-400" :line "rgba(74,222,128,0.25)" :ring "ring-green-400/40")
      (session-color :dot "bg-amber-400" :line "rgba(251,191,36,0.25)" :ring "ring-amber-400/40")
      (session-color :dot "bg-pink-400" :line "rgba(244,114,182,0.25)" :ring "ring-pink-400/40")
      (session-color :dot "bg-violet-400" :line "rgba(167,139,250,0.25)" :ring "ring-violet-400/40")
      (session-color :dot "bg-orange-400" :line "rgba(251,146,60,0.25)" :ring "ring-orange-400/40")
      (session-color :dot "bg-teal-300" :line "rgba(94,234,212,0.25)" :ring "ring-teal-300/40")
      (session-color :dot "bg-rose-400" :line "rgba(251,113,133,0.25)" :ring "ring-rose-400/40")
      (window-option :label "5m" :value "5min")
      (window-option :label "10m" :value "10min")
      (window-option :label "30m" :value "30min")
      (window-option :label "1h" :value "1h")
      (window-option :label "6h" :value "6h")
      (window-option :label "24h" :value "24h")))

  (pillar-flow-map
    :schema "missiond.frontend.pillar-flow-map.v1"
    :rule "Each frontend pillar owns functions; each function declares entry -> ordered core steps -> egress and maps to one implementation surface."

    (pillar app-shell
      (function shell-navigation
        :surface app-shell
        :entry [browser-load tab-click localStorage board-store eventbus-state]
        :core ((step s1 :logic "mount the app shell and establish the global EventBus subscription")
               (step s2 :logic "restore tab and selected slot from localStorage with legacy tab migration")
               (step s3 :logic "fetch BoardTask summary and runtime-projected slots")
               (step s4 :logic "compose the selected dashboard without changing shared task/runtime state")
               (step s5 :logic "persist only local view preferences back to localStorage"))
        :egress [dashboard-view selected-tab selected-slot eventbus-indicator]))

    (pillar missiond-proxy
      (function api-proxy-runtime
        :surface missiond-proxy
        :entry [next-api-route callTool callMissiond browser-fetch]
        :core ((step s1 :logic "resolve the MissionD IPC socket from env or default paths")
               (step s2 :logic "forward tool calls as JSON-RPC tools/call with a bounded timeout")
               (step s3 :logic "parse tool text payloads as JSON for browser routes")
               (step s4 :logic "return typed route responses or explicit 502 errors")
               (step s5 :logic "mark runtime-only MissionD IPC routes as Next.js force-dynamic nodejs routes so build-time page-data collection never calls the daemon"))
        :egress [json-response route-error missiond-tool-result]))

    (pillar board-task-ui
      (function board-task-state
        :surface board-task-ui
        :entry [TaskDialog QuickAdd TaskItem TaskFilters BoardConsolidated useTaskCenterStore]
        :core ((step s1 :logic "load BoardTasks through the tasks API and preserve optimistic local edits")
               (step s2 :logic "optimistic create/update/delete/toggle/skip/reorder mutations must either reconcile to the API response or roll back the affected local task snapshot on failure")
               (step s3 :logic "create, update, delete, toggle, skip, reorder, and note BoardTasks through the API adapter")
               (step s4 :logic "derive filters, grouping, hidden/skipped visibility, parent-child ordering, and flow badges")
               (step s5 :logic "use runtime-projected slot choices when a user manually assigns Autopilot work"))
        :egress [task-list task-dialog-state task-api-mutation task-note]))

    (pillar workstation-terminal-ui
      (function workstation-terminal
        :surface workstation-terminal-ui
        :entry [slots-api mission_pty_status mission_master_status pty-websocket Terminal AutopilotMonitor]
        :core ((step s1 :logic "load slot rows from MissionD runtime projection and PTY recognition snapshots")
               (step s2 :logic "normalize PTY state case; sort running slots first, then active idle sessions, then terminal sessions")
               (step s3 :logic "render Terminal as a raw PTY/slot viewer: provider-grouped slot rail on the left, selected PTY in the center, evidence/diagnostics rail on the right")
               (step s4 :logic "keep dynamic slot labels bounded and truncated; long labels must not wrap or occlude the terminal")
               (step s5 :logic "connect xterm to the selected PTY websocket only when the slot is running")
               (step s6 :logic "render provider-neutral state, spawn, reconnect, stop, screenshot, MCP readiness, and blocked-status controls")
               (step s7 :logic "show Autopilot slot/task correlation from activeBoardTaskId/currentTaskId or running BoardTask claimExecutorId/assignee without owning dispatch or closure")
               (step s8 :logic "show latest durable provider conversation, recognition confidence/reason, MCP readiness, active tool, and blocked kind as diagnostics beside the PTY")
               (step s9 :logic "for stopped/exited/complete slots, keep the slot selectable and show latest durable conversation metadata instead of a blank PTY screen")
               (step s10 :logic "slot refresh is driven by EventBus slotVersion invalidation; interval polling is only a bounded fallback with diagnostic intent"))
        :egress [provider-slot-rail terminal-screen pty-state evidence-diagnostics autopilot-monitor]))

    (pillar execution-cockpit-ui
      (function execution-cockpit
        :surface execution-cockpit-ui
        :entry [BoardTasks slots-api mission_pty_status mission_master_status eventbus-websocket ExecDashboard Terminal]
        :core ((step s1 :logic "render Exec as an orchestration cockpit rather than a second Terminal surface")
               (step s2 :logic "start from active BoardTasks, then join each task to claimExecutorId/assignee/currentTaskId/activeBoardTaskId slot evidence")
               (step s3 :logic "show BoardTask status, dispatch/evidence/checkpoint diagnostics, latest durable conversation, and EventBus wait state")
               (step s4 :logic "derive execution-step-digest from BoardTask description, recent notes, slot state, and durable conversation metadata so operators can flip through the work trace without reading raw PTY")
               (step s5 :logic "embed the selected PTY as a detail panel for diagnosis only; durable provider logs and MissionD events remain completion authority")
               (step s6 :logic "surface stale/future timestamps, stopped PTY with durable conversation fallback, and missing MCP readiness as diagnostics"))
        :egress [execution-queue execution-step-digest evidence-panel eventbus-wait-state pty-detail diagnostics]))

    (pillar event-stream-ui
      (function event-stream-cache
        :surface event-stream-ui
        :entry [eventbus-websocket connected caught_up too_far_behind FrontendEvent]
        :core ((step s1 :logic "connect to the EventBus websocket using configured host and port")
               (step s2 :logic "sync missed events using last sequence and request resync on large gaps")
               (step s3 :logic "route event types to debounced domain version counters")
               (step s4 :logic "dispatch local CustomEvents only for UI-local updates such as timeline summaries and Jarvis completion")
               (step s5 :logic "reconnect with bounded exponential backoff"))
        :egress [connection-state version-counters health-snapshot ui-custom-event]))

    (pillar timeline-log-ui
      (function timeline-logs
        :surface timeline-log-ui
        :entry [timeline-api timeline-store CognitiveTimeline LogsConsolidated Conversations]
        :core ((step s1 :logic "fetch events, traces, conversations, transcripts, and system logs through API routes")
               (step s2 :logic "compute timeline lane layout for chat, slot, board, system, and execution events")
               (step s3 :logic "render selection, detail panels, summaries, markdown, JSON, and tool views")
               (step s4 :logic "react to event-stream version bumps without directly mutating backend state")
               (step s5 :logic "conversation views must not render duplicate same-session messages with identical uuid or identical role/timestamp/content fallback")
               (step s6 :logic "render worker_user as a distinct worker input role while preserving rawRole for provider-audit diagnostics"))
        :egress [timeline-view logs-view conversation-view selection-state]))

    (pillar knowledge-system-ui
      (function knowledge-system-dashboards
        :surface knowledge-system-ui
        :entry [KnowledgeConsolidated SystemDashboard DecisionDashboard ArchitectureView api-routes mission_infra_query]
        :core ((step s1 :logic "fetch KB, memory, architecture, system health, deploy, infrastructure-universe, and model-trace projections")
               (step s2 :logic "normalize read-only dashboard state for dense operator scanning")
               (step s3 :logic "render SSOT Universe, service runtime deployment/domain/DNS capability, runtime targets, credential refs, skill evidence drift, Decision Inbox, memory review, runtime health, architecture graph, status badges, and diagnostics")
               (step s4 :logic "keep operational actions behind explicit buttons and existing route policies")
               (step s5 :logic "project constants promoted from repeated KB lookup, such as auth service path/ports and 12900kf/router target anchors, into the Universe/Infra surface instead of leaving them as loose memory")
               (step s6 :logic "JarvisChat sends user text/images through master-chat-api to create a visible resident-master-control BoardTask; it polls mission_master_status and never writes directly to a PTY"))
        :egress [knowledge-dashboard system-dashboard architecture-dashboard operational-action]))

    (pillar frontend-design-system
      (function board-design-system
        :surface frontend-design-system
        :entry [tailwind globals ui-components icon-buttons]
        :core ((step s1 :logic "use shared primitives for buttons, inputs, selects, dialogs, labels, badges, and skeletons")
               (step s2 :logic "preserve dense dashboard layout and existing color semantics")
               (step s3 :logic "keep responsive constraints stable for tabs, terminal, task lists, timelines, and cards")
               (step s4 :logic "avoid feature-instruction prose in the app surface; reserve explanation for docs and evidence sidecars"))
        :egress [consistent-ui responsive-layout accessible-controls])))

  (implementation-map
    (surface app-shell
      :status "code-aligned"
      :implements [shell-navigation]
      :code ["packages/board/src/App.tsx"
             "packages/board/src/generated/board-frontend-config.ts"
             "scripts/project-frontend-board-config.mjs"
             "packages/board/src/app/page.tsx"
             "packages/board/src/app/layout.tsx"])
    (surface missiond-proxy
      :status "code-aligned"
      :implements [api-proxy-runtime]
      :code ["packages/board/src/lib/missiond.ts"
             "packages/board/src/app/api/tasks/route.ts"
             "packages/board/src/app/api/slots/route.ts"
             "packages/board/src/app/api/master/status/route.ts"
             "packages/board/src/app/api/pty/status/route.ts"
             "packages/board/src/app/api/pty/spawn/route.ts"
             "packages/board/src/app/api/pty/kill/route.ts"
             "packages/board/src/app/api/pty/confirm/route.ts"])
    (surface board-task-ui
      :status "code-aligned"
      :implements [board-task-state]
      :code ["packages/board/src/types.ts"
             "packages/board/src/api.ts"
             "packages/board/src/store.ts"
             "packages/board/src/constants.ts"
             "packages/board/src/generated/board-frontend-config.ts"
             "packages/board/src/components/BoardConsolidated.tsx"
             "packages/board/src/components/TaskDialog.tsx"
             "packages/board/src/components/TaskItem.tsx"
             "packages/board/src/components/TaskListView.tsx"
             "packages/board/src/components/TaskFilters.tsx"
             "packages/board/src/components/QuickAdd.tsx"
             "packages/board/src/components/PendingQuestions.tsx"])
    (surface workstation-terminal-ui
      :status "code-aligned"
      :implements [workstation-terminal]
      :code ["packages/board/src/components/Terminal.tsx"
             "packages/board/src/components/AutopilotMonitor.tsx"
             "packages/board/src/app/api/slots/route.ts"
             "packages/board/src/app/api/pty/status/route.ts"
             "packages/board/src/app/api/pty/screen/route.ts"
             "packages/board/src/app/api/pty/agents/route.ts"])
    (surface execution-cockpit-ui
      :status "code-aligned"
      :implements [execution-cockpit]
      :code ["packages/board/src/components/ExecDashboard.tsx"
             "packages/board/src/components/Terminal.tsx"
             "packages/board/src/App.tsx"
             "packages/board/src/app/api/slots/route.ts"
             "packages/board/src/eventStream.ts"])
    (surface event-stream-ui
      :status "code-aligned"
      :implements [event-stream-cache]
      :code ["packages/board/src/eventStream.ts"
             "packages/board/src/generated/board-frontend-config.ts"
             "packages/board/src/hooks/useEventStream.ts"])
    (surface timeline-log-ui
      :status "code-aligned"
      :implements [timeline-logs]
      :code ["packages/board/src/components/timeline/index.ts"
             "packages/board/src/components/timeline/CognitiveTimeline.tsx"
             "packages/board/src/components/timeline/constants.tsx"
             "packages/board/src/components/timeline/helpers.ts"
             "packages/board/src/components/timeline/stores/timelineStore.ts"
             "packages/board/src/components/LogsConsolidated.tsx"
             "packages/board/src/components/Conversations.tsx"
             "packages/board/src/app/api/timeline/events/route.ts"
             "packages/board/src/app/api/conversations/route.ts"
             "packages/board/src/app/api/transcripts/route.ts"])
    (surface knowledge-system-ui
      :status "code-aligned"
      :implements [knowledge-system-dashboards]
      :code ["packages/board/src/components/KnowledgeConsolidated.tsx"
             "packages/board/src/components/SystemDashboard.tsx"
             "packages/board/src/components/DecisionDashboard.tsx"
             "packages/board/src/components/architecture/ArchitectureView.tsx"
             "packages/board/src/components/JarvisChat.tsx"
             "packages/board/src/app/api/kb/route.ts"
             "packages/board/src/app/api/infra/route.ts"
             "packages/board/src/app/api/questions/route.ts"
             "packages/board/src/app/api/system/health/route.ts"
             "packages/board/src/app/api/jarvis/conversations/route.ts"
             "packages/board/src/app/api/master/chat/route.ts"
             "packages/board/src/app/api/master/status/route.ts"])
    (surface frontend-design-system
      :status "code-aligned"
      :implements [board-design-system]
      :code ["packages/board/src/app/globals.css"
             "packages/board/src/components/ui/button.tsx"
             "packages/board/src/components/ui/input.tsx"
             "packages/board/src/components/ui/select.tsx"
             "packages/board/src/components/ui/dialog.tsx"
             "packages/board/src/components/ui/textarea.tsx"
             "packages/board/src/components/ui/badge.tsx"
             "packages/board/src/components/ui/skeleton.tsx"]))

  (workstation-dispatch-plan
    :strategy two-stage-context-pack
    :investigation-fanout 12
    :implementation-fanout 6
    :same-file-owner single
    :read-only-workers [gemini-ultra codex-cli claude-code-default]
    :coding-workers [claude-code-default]
    :rules ["Investigation workers may only append context-pack entries."
            "Implementation workers must receive context-pack path, write_scope, must_not_touch, acceptance, model_profile, timeout_secs."
            "Gemini remains read-only until a scoped-commit smoke proves it can write without stage pollution."
            "Every dispatch anomaly becomes a Lisp rule, checker fixture, or runtime guard."])

  (quality-gates
    :checks ["node scripts/check-frontend-board-lisp-schema.mjs"
             "node scripts/project-frontend-board-config.mjs --check"
             "node scripts/check-frontend-board-code-isomorphism.mjs"
             "node scripts/check-frontend-board-runtime-projection.mjs"
             "pnpm --dir packages/board build"]))
