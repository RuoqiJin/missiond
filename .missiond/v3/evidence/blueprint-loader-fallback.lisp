(missiond-v3-blueprint-loader-fallback-evidence
  :schema "missiond.v3.blueprint-loader-fallback.v1"
  :source "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
  :board-task "8d5aa2b6-9f59-415d-973e-ef28afa897c2"
  :rule "Loader file-resolution mechanics for V3 blueprints. The blueprint *content* schema is unchanged; this records the precedence used to *locate* the source file when a target project lacks its own per-project override."

  (resolution-order
    :id loader-001
    :context "v3_blueprint_runtime::load_blueprint_source"
    :ordered-steps [
      "1. target_project_root/.missiond/v3/missiond-blueprint.lisp (per-project override)"
      "2. orchestrator blueprint via locate_orchestrator_blueprint:"
      "   2a. $MISSIOND_ORCHESTRATOR_ROOT/.missiond/v3/missiond-blueprint.lisp"
      "   2b. cwd ancestor walk for .missiond/v3/missiond-blueprint.lisp"
      "   2c. /Users/jinchen/Projects/missiond/.missiond/v3/missiond-blueprint.lisp (last-resort hardcoded, parity with main.rs:953 + universe.rs::locate_v3_blueprint)"
      "3. None -> caller falls back to Self::default() (preserved test/CLI compatibility)"
    ]
    :applies-to [
      "WorkstationRuntimeConfig"
      "FlowRuntimeConfig"
      "ComputePrimitivesRuntimeConfig"
      "MinimaxRuntimeConfig"
      "RouterRuntimeConfig"
      "CascadeRuntimeConfig"
      "ProjectRegistryRuntimeConfig"
      "CapabilityGovernanceRuntimeConfig"
      "MemoryKbRuntimeConfig"
      "ConversationIngestionRuntimeConfig"
      "AutopilotRuntimeConfig"
      "LearningEngineRuntimeConfig"
    ])

  (failure-modes
    :id loader-002
    :context "before/after delta — what changed for dispatch to a registered external project that has .missiond/intent.lisp (V2) but no v3 blueprint"
    :before {
      :case-1 "target/.missiond/v3/missiond-blueprint.lisp exists -> use target's"
      :case-2 "target/.missiond/ exists, no v3 file -> Err(MissingBlueprint) (fail dispatch)"
      :case-3 "no .missiond/ at target -> Ok(Self::default()) (silent embedded fallback)"
      :case-4 "project_root = None -> Ok(Self::default()) (silent embedded fallback)"
    }
    :after {
      :case-1 "unchanged: target's blueprint preserved as per-project override"
      :case-2 "now falls back to orchestrator blueprint (SSOT inheritance) — unblocks xiaojinpro-backend dispatch"
      :case-3 "now falls back to orchestrator blueprint instead of silent defaults"
      :case-4 "now falls back to orchestrator blueprint instead of silent defaults"
      :case-5 "neither target nor orchestrator blueprint locatable -> Ok(Self::default()) (only path that returns embedded defaults; reserved for test/CLI environments without a MissionD installation)"
    }
    :upstream-fail-fast "Unregistered cwd dispatch is rejected earlier in slot_orchestrator::project_root::resolve_target_project_root with CwdOutsideRegisteredProject. The V3 loader change does not weaken that boundary; it only fixes the resolution that runs *after* a registered target was resolved.")

  (waiver
    :id loader-003
    :reason "lisp-isomorphism axiom binds handlers to *named Lisp contracts*; this change has no contract delta (no new field, parser, schema, or declared policy). It only rewires the file-discovery substrate beneath unchanged contracts. The blueprint at .missiond/v3/missiond-blueprint.lisp itself is unmodified."
    :coverage "Existing v3_blueprint_runtime tests at crates/missiond-daemon/src/context/v3_blueprint_runtime.rs::tests (27 tests) parse against fixture sources, decoupled from the loaders, and continue to pass. project_root resolver tests (28 tests) likewise unaffected. End-to-end verification is the dispatch unblock recorded against board task 8d5aa2b6-9f59-415d-973e-ef28afa897c2."
    :followups [
      "Consolidate the hardcoded /Users/jinchen/Projects/missiond fallback (now appearing in main.rs:953, universe.rs:101, v3_blueprint_runtime.rs::locate_orchestrator_blueprint) behind a single canonical resolver in a separate change."
      "Daemon restart required for runtime effect (see ops_missiond_restart memory)."
    ]))
