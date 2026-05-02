(missiond-frontend-blueprint
  :schema "missiond.frontend-blueprint.v1"
  :project board
  :app "@missiond/board"
  :root "packages/board"
  :authority "project-local-lisp-ssot"
  :parent-v3 ".missiond/v3/missiond-blueprint.lisp"
  :status "code-aligned"
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
      :fields [id label role running state provider engine modelProfile taskClass acceptsBoardTask confidence reason activeTool blockedKind latestConversation]
      :forbid [SLOT_OPTIONS hardcoded-sonnet-label])
    (projection pty-recognition
      :source [mission_pty_status provider-aware-recognition]
      :entry ["packages/board/src/components/Terminal.tsx"]
      :fields [state statusText blockedKind provider confidence]
      :rule "Terminal labels must describe the selected provider/session generically; no Claude-only copy in shared PTY surfaces.")
    (projection eventbus-cache-invalidation
      :source [eventbus-ws]
      :entry ["packages/board/src/eventStream.ts"]
      :fields [seq type payload version-counter custom-event])
    (projection board-task-contract
      :source [mission_board mission_task_delegate]
      :entry ["packages/board/src/api.ts"
              "packages/board/src/store.ts"
              "packages/board/src/app/api/tasks/route.ts"]
      :fields [status priority category assignee autoExecute promptTemplate flowTemplate dependsOn leaseExpiresAt notes]))

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
      (prefix-route :prefix "narration_" :bump [timelineVersion] :delay-ms 500)))

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
               (step s4 :logic "return typed route responses or explicit 502 errors"))
        :egress [json-response route-error missiond-tool-result]))

    (pillar board-task-ui
      (function board-task-state
        :surface board-task-ui
        :entry [TaskDialog QuickAdd TaskItem TaskFilters BoardConsolidated useTaskCenterStore]
        :core ((step s1 :logic "load BoardTasks through the tasks API and preserve optimistic local edits")
               (step s2 :logic "create, update, delete, toggle, skip, reorder, and note BoardTasks through the API adapter")
               (step s3 :logic "derive filters, grouping, hidden/skipped visibility, parent-child ordering, and flow badges")
               (step s4 :logic "use runtime-projected slot choices when a user manually assigns Autopilot work"))
        :egress [task-list task-dialog-state task-api-mutation task-note]))

    (pillar workstation-terminal-ui
      (function workstation-terminal
        :surface workstation-terminal-ui
        :entry [slots-api mission_pty_status pty-websocket Terminal ExecDashboard AutopilotMonitor]
        :core ((step s1 :logic "load slot rows from MissionD runtime projection and PTY recognition snapshots")
               (step s2 :logic "sort running slots first and preserve the selected slot when still present")
               (step s3 :logic "connect xterm to the selected PTY websocket only when the slot is running")
               (step s4 :logic "render provider-neutral state, spawn, reconnect, stop, screenshot, and blocked-status controls")
               (step s5 :logic "show Autopilot slot/task correlation without owning dispatch or closure"))
        :egress [slot-list terminal-screen pty-state autopilot-monitor]))

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
               (step s4 :logic "react to event-stream version bumps without directly mutating backend state"))
        :egress [timeline-view logs-view conversation-view selection-state]))

    (pillar knowledge-system-ui
      (function knowledge-system-dashboards
        :surface knowledge-system-ui
        :entry [KnowledgeConsolidated SystemDashboard ArchitectureView api-routes]
        :core ((step s1 :logic "fetch KB, memory, architecture, system health, deploy, and model-trace projections")
               (step s2 :logic "normalize read-only dashboard state for dense operator scanning")
               (step s3 :logic "render cards, tables, architecture graph, status badges, and diagnostics")
               (step s4 :logic "keep operational actions behind explicit buttons and existing route policies"))
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
             "packages/board/src/components/ExecDashboard.tsx"
             "packages/board/src/components/AutopilotMonitor.tsx"
             "packages/board/src/app/api/slots/route.ts"
             "packages/board/src/app/api/pty/status/route.ts"
             "packages/board/src/app/api/pty/screen/route.ts"
             "packages/board/src/app/api/pty/agents/route.ts"])
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
             "packages/board/src/components/architecture/ArchitectureView.tsx"
             "packages/board/src/components/JarvisChat.tsx"
             "packages/board/src/app/api/kb/route.ts"
             "packages/board/src/app/api/system/health/route.ts"
             "packages/board/src/app/api/jarvis/conversations/route.ts"])
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
