;; MissionD — Pillar: standalone
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

  (pillar standalone
    (purpose "independently deployable services and client packages")

    (component skill-store
      :target "crates/skill-store/src/main.rs"
      (routes
        (auth          :target "crates/skill-store/src/routes/auth.rs")
        (skills        :target "crates/skill-store/src/routes/skills.rs")
        (invoke        :target "crates/skill-store/src/routes/invoke.rs")
        (creator       :target "crates/skill-store/src/routes/creator.rs")
        (subscriptions :target "crates/skill-store/src/routes/subscriptions.rs"))
      (services
        (billing   :target "crates/skill-store/src/services/billing.rs")
        (defense   :target "crates/skill-store/src/services/defense.rs")
        (executor  :target "crates/skill-store/src/services/executor.rs")
        (llm-proxy :target "crates/skill-store/src/services/llm_proxy.rs")))

    (component missiond-attach
      :target "crates/missiond-attach/src/main.rs")

    (component missiond-runner
      :target "crates/missiond-runner/src/runner.rs")

    (component semantic-terminal-napi
      :target "crates/semantic-terminal-napi/src/lib.rs")

    (component board-frontend
      :target "packages/board/src/App.tsx"
      (api-routes :target "packages/board/src/app/api/"
        (route "/api/slots"            :handler "slots/route.ts")
        (route "/api/tasks"            :handler "tasks/route.ts")
        (route "/api/conversations"    :handler "conversations/route.ts"
          :params (status limit conversationType source project)
          :note "project参数透传至mission_conversation_list; commit 5671c95")
        (route "/api/kb"               :handler "kb/route.ts"
          :methods (GET DELETE PATCH)
          :params (query category project limit)
          :note "project参数透传至mission_kb_query/mission_kb_list; commit 5671c95"
          (method PATCH
            :added "3c10d21"
            :body "{key, project_id}"
            :action "kb_update(key, project_id) — 空串清除归属"
            :returns "ToolResult from mission_kb_update"))
        (route "/api/projects"         :handler "projects/route.ts"
          :method GET :calls "mission_project{action:list}"
          :returns "Vec<ProjectInfo>(含lispFiles/lispCount/githubUrl)"
          :added "eae9bbd")
        (route "/api/questions"        :handler "questions/route.ts")
        (route "/api/timeline/events"  :handler "timeline/events/route.ts")
        (route "/api/architecture"     :handler "architecture/route.ts")
        (route "/api/pty/spawn"        :handler "pty/spawn/route.ts")
        (route "/api/pty/screen"       :handler "pty/screen/route.ts")
        (route "/api/pty/confirm"      :handler "pty/confirm/route.ts")
        (route "/api/system/health"    :handler "system/health/route.ts")
        (route "/api/system/llm-traces" :handler "system/llm-traces/route.ts")
        (route "/api/deploy/status"    :handler "deploy/status/route.ts"))

      (ui-components :target "packages/board/src/components/"
        (component Conversations
          :target "packages/board/src/components/Conversations.tsx"
          (state projectFilter :type string :default "all")
          (state projects      :type "Project[]" :fetched-from "/api/projects")
          (widget project-filter-select
            :trigger onMount+onChange :passes "project" to "/api/conversations"
            :shows active-projects-sorted-by-conversation_count
            :added "5671c95")
          (note "viewMode union type 新增 jarvis — fix pre-existing build error (commit ac96b3c)"))

        (component KnowledgeBase
          :target "packages/board/src/components/KnowledgeBase.tsx"
          (state activeProject :type "string|null" :default nil)
          (state projects      :type "Project[]" :fetched-from "/api/projects")
          (widget project-filter-pills
            :style pill-buttons :separator divider-line
            :passes "project" to "/api/kb"
            :added "5671c95"
            :note "commit 3c10d21: '__unclassified__' pill为客户端过滤(不传后端), filter: !e.projectId")
          (widget per-entry-project-selector
            :added "3c10d21"
            :location "KBEntryCard — 每张卡片底部 meta 行"
            :type "<select> 下拉"
            :options ("未分类(value='')" "各 ProjectConfig.id")
            :style "无项目: border-neutral-700/text-neutral-500; 有项目: border-cyan-500/30 text-cyan-400"
            :interaction "onChange → handleSetProject(key, projectId|null) → PATCH /api/kb → 乐观更新state; 失败时fetchEntries回滚"))

        (component SystemDashboard
          :target "packages/board/src/components/SystemDashboard.tsx"
          (sub-component ProjectsPanel
            :target "packages/board/src/components/SystemDashboard.tsx"
            :fetches "/api/projects"
            :layout "grid 1/2/3 columns responsive"
            :sort "active first → lispCount desc → alphabetical"
            :card-fields (id path-abbreviated active-badge githubUrl-link slots lisp-files-tags)
            :note "githubUrl: git@github.com:X/Y.git → https://github.com/X/Y 转换"
            :added "eae9bbd")
          :note "ProjectsPanel 置于 MemoryDashboard 之前(最顶部折叠区); collapsed 默认展开")))

    (component node-client
      :target "packages/node-client/src/index.ts"
      (modules
        (client :target "packages/node-client/src/client.ts")
        (daemon :target "packages/node-client/src/daemon.ts")
        (binary :target "packages/node-client/src/binary.ts")
        (pty    :target "packages/node-client/src/pty.ts")
        (types  :target "packages/node-client/src/types.ts"))))

