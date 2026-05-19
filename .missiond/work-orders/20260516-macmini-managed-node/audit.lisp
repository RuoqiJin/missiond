(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "20260516-macmini-managed-node"
  :events ((event created
              :at "2026-05-16T10:39:48.207Z"
              :actor missiond-work-order)
           (event committed-portable-root-runtime
              :at "2026-05-16T18:42:00+08:00"
              :commit "9cca7593"
              :summary "MissionD runtime root resolution now uses MISSIOND_PROJECT_ROOT / MISSIOND_ORCHESTRATOR_ROOT before home fallback; Mac mini path no longer depends on /Users/jinchen.")
           (event installed-macmini-runtime
              :at "2026-05-17T22:21:00+08:00"
              :host "rickyhqmac-mini"
              :project-root "/Users/rickyhq/Projects/missiond"
              :runtime-root "/Users/rickyhq/.xjp-mission"
              :database "postgres://rickyhq@127.0.0.1:5433/missiond"
              :health "http://127.0.0.1:9120/health ok")
           (event verified-macmini-build
              :at "2026-05-17T22:20:00+08:00"
              :host "rickyhqmac-mini"
              :checks ("cargo check -p missiond-mcp"
                       "cargo build -p missiond-mcp"
                       "cargo build -p missiond-daemon -p missiond-mcp --release"
                       "mission-mcp initialize smoke")
              :result "passed")
           (event configured-macmini-workstations
              :at "2026-05-17T22:30:00+08:00"
              :host "rickyhqmac-mini"
              :providers ((claude-code :cli "2.1.143" :mcp "missiond connected" :auth "needs-login")
                          (codex :cli "0.130.0" :mcp "missiond enabled" :auth "logged-in")
                          (gemini :cli "0.42.0" :mcp "not-configured" :auth "oauth-file-present"))
              :slots "10 projected slots: 5 claude_code, 4 gemini, 1 codex")
           (event known-followups
              :at "2026-05-17T22:31:00+08:00"
              :items ("Configure GitHub private repository credentials on Mac mini before switching source updates from archive sync to git pull."
                      "Complete ClaudeCode subscription login on Mac mini before using ClaudeCode as primary coding worker."
                      "Decide whether Gemini should also receive MissionD MCP config; current MissionD worker model does not require it."))
           (event managed-slot-smoke
              :at "2026-05-18T00:18:00+08:00"
              :host "rickyhqmac-mini"
              :result "managed dynamic slot created and spawned, then ClaudeCode exited before useful work"
              :evidence ("slot-dyn-f49a8c75 created from MissionD MCP"
                         "provider warning: missing /Users/rickyhq/.claude/.credentials.json"
                         "launch log showed stale /Users/jinchen/.xjp-mission/xjp-mcp-config.json path"
                         "daemon hit clean-node dynamic_slots ttl_seconds int4/i64 decode panic"))
           (event patched-managed-node-failure-class
              :at "2026-05-18T00:32:00+08:00"
              :summary "PTY now preserves auth_missing / billing_or_account / usage_limit as blocked provider-unavailable diagnostics across Exited/Error; dynamic slot decode accepts int4; PTY launch resolves host-local MCP config paths.")
           (event local-build-lane-decision
              :at "2026-05-18T13:45:00+08:00"
              :summary "Mac mini is powerful enough to build MissionD locally; steady-state update should use XJP codebase sync plus on-target cargo build/test/install instead of large direct binary scp."
              :preferred_lane macmini-codebase-local-build-lane
              :bootstrap_status "daemon health is reachable, but this turn's direct large-binary transfer was aborted because it hung before replacing binaries; current remote health remains on existing installed binary.")
           (event remaining-provider-action
              :at "2026-05-18T00:33:00+08:00"
              :items ("Resolve ClaudeCode billing/subscription state for the Mac mini account before expecting ClaudeCode worker execution."
                      "After subscription recovers, rerun managed slot smoke and confirm Terminal shows blocked/running/final evidence rather than a silent complete exit."))))
