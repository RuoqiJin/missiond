;; Worker report for wave33-01-autopilot-prompt-contract-v0.
;; Schema: missiond.report-contract.v1

(report wave33-01-autopilot-prompt-contract-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave33-01-autopilot-prompt-contract-v0"
  :status done
  :commit_hash "9245d4268a2b2521a44a98b201539f41252ba033"
  :files_changed
    ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
     ".missiond/v3/missiond-blueprint.lisp"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon engine::intent_engine::autopilot::tests"
      :exit_code 0 :ok true
      :detail "18 tests passed (10 pre-existing build_stuck_summary + pty/watchdog policy + 8 new prompt-contract tests); 1704 filtered.")
     (:command "cargo check -p missiond-daemon"
      :exit_code 0 :ok true
      :detail "compiles clean; no new warnings introduced by autopilot.rs (pre-existing crate dead-code/unused-import warnings unchanged).")
     (:command "node scripts/check-lisp-blueprint-compression.mjs"
      :exit_code 0 :ok true
      :detail "v1 manifest + v3 blueprint compression check OK after workstation-config prompt-tool-contract addition + implementation-map note update.")
     (:command "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "architecture-lisp shape OK (1 file scanned).")
     (:command "perl -ne 'exit 1 if /\\x00/' crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "no NUL bytes in either write-scope file.")
     (:command "git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "no trailing-whitespace / mixed-indent diff complaints in the staged write scope.")]

  :blueprint_change
    (:file ".missiond/v3/missiond-blueprint.lisp"
     :workstation_config_prompt_tool_contract_added true
     :prompt_tool_contract_form
       (prompt-tool-contract autopilot-claudecode-prompt
         :applies-to [coder researcher ops]
         :always-shown
           ["Board Task ID is surfaced in every dispatched prompt so the worker (and any reader of the prompt snapshot) can audit which BoardTask the slot is executing."]
         :objective-dedupe
           "Prompt assembly MUST suppress duplicated objective text: when BoardTask.description equals BoardTask.title or starts with the title followed only by blank lines, the assembled prompt uses description alone; distinct title + description still renders both joined by a blank line."
         :board-self-close
           (:mode conditional
            :when-tools-present "If mission_board_update and mission_board_note_add MCP tools are attached to the slot, the worker SHOULD call mission_board_update(status=\"done\") and mission_board_note_add(noteType=\"summary\") when the task completes."
            :when-tools-absent "If those board tools are not attached to the slot, the worker MUST instead return a concise final completion summary; Autopilot/orchestrator remains responsible for closing the BoardTask."
            :rationale "ClaudeCode slots may run with reduced MCP surfaces; an unconditional must-call instruction makes such slots unable to honor the prompt and leaks orchestrator state into the worker contract.")
         :non-prompt-guidance
           ["Decision Engine escalation suffix (mission_question_create) and ops-task focus suffix remain behaviorally intact and are appended after the deduplicated base prompt."])
     :implementation_map_note_changed true
     :implementation_map_note_summary
       "workstation-config surface :note now records that Autopilot prompt assembly projects the V3 prompt-tool-contract via build_base_prompt (objective dedupe) and append_board_task_id_suffix (conditional board self-close); the prompt no longer hardcodes mission_board_update / mission_board_note_add as unconditional must-calls."
     :status_kept "code-aligned-partial"
     :status_rationale "wave33-01 brings runtime prompt/tool wording into Lisp/projection alignment; remaining workstation-config debt (model-profile cascade, idle-slot reuse) is unchanged.")

  :prompt_helper_behavior
    (:helper "build_base_prompt(title: &str, description: &str) -> String"
     :rules
       ["description empty → return title alone (single-line objective path)."
        "description starts with title followed by whitespace or end-of-string → return description as-is (covers description == title, \"title\\n\\n\", \"title\\n\\nbody\", and \"title body\")."
        "description does not start with the exact title token at a word boundary → return \"{title}\\n\\n{description}\" (preserves the original distinct-detail rendering)."]
     :word-boundary-guard "Dedupe only fires when the title is followed by a whitespace char or end-of-string, so a title like \"Fix\" is not treated as a prefix of \"Fixing CORS\"."
     :test_coverage
       [(:name "build_base_prompt_empty_description_returns_title_alone"
         :proves "Empty description renders title alone.")
        (:name "build_base_prompt_description_equal_to_title_drops_duplicate"
         :proves "description == title → output equals title with exactly one occurrence (no doubling).")
        (:name "build_base_prompt_description_starts_with_title_then_blank_lines_only"
         :proves "task_delegate's `objective + trailing blank` path is not re-prepended; the title appears exactly once.")
        (:name "build_base_prompt_distinct_description_keeps_both"
         :proves "Distinct title + description still render both joined by a blank line.")
        (:name "build_base_prompt_title_prefix_with_extra_body_keeps_description_intact"
         :proves "Description that starts with title followed by blank lines and real body keeps the description as-is and shows the title only once.")])

  :board_self_close_helper
    (:helper "append_board_task_id_suffix(prompt: &str, task_id: &str) -> String"
     :always_surfaces "📋 **Board Task ID**: `<task_id>` for audit linkage, regardless of MCP tool attachment."
     :replacement_wording_summary
       "Old wording said the worker MUST call mission_board_update / mission_board_note_add. New wording is conditional on tool attachment and explicitly authorizes returning a final summary when those tools are absent."
     :exact_replacement_wording
       (:old_zh "任务完成后，你必须调用 `mission_board_update(id=\"<task_id>\", status=\"done\")` 关闭此任务，并用 `mission_board_note_add(taskId=\"<task_id>\", content=\"...\", noteType=\"summary\")` 写入诊断摘要。"
        :new_zh "任务完成时：若当前工位已挂载 `mission_board_update` / `mission_board_note_add`，请调用 `mission_board_update(id=\"<task_id>\", status=\"done\")` 关闭任务，并用 `mission_board_note_add(taskId=\"<task_id>\", content=\"...\", noteType=\"summary\")` 写入诊断摘要。若上述 board MCP 工具未挂载到本工位，请直接返回一段简明的最终完成摘要，由 Autopilot/orchestrator 负责关闭此 BoardTask。")
     :test_coverage
       [(:name "append_board_task_id_suffix_surfaces_board_task_id"
         :proves "Suffix includes the literal `Board Task ID` label and the backticked task id, regardless of tool availability.")
        (:name "append_board_task_id_suffix_is_conditional_not_unconditional"
         :proves "Old `你必须调用` wording is gone, conditional `若当前工位已挂载` clause is present, both tool names are still mentioned, and the tools-absent fallback explicitly tells the worker to return a final summary while keeping Autopilot/orchestrator responsible for closing the BoardTask.")])

  :preserved_behavior
    ["Decision Engine escalation suffix (mission_question_create) — unchanged in autopilot.rs and re-stated in the V3 prompt-tool-contract :non-prompt-guidance."
     "Ops-task focus suffix (mission_reachability / mission_os_diagnose) — unchanged."
     "Predecessor task handover injection, answered-question injection, KB context prefetch, prompt snapshot persistence — all unchanged."
     "Watchdog / pty timeout policy from wave32-01 — unchanged."]

  :notes
    "Pure helpers (build_base_prompt / append_board_task_id_suffix) take owned slices and return new String, so unit tests need no AppState. Dedupe rule uses the simplest correct word-boundary check (rest is empty OR starts with whitespace), which covers all four delegated-path shapes (title-only, title==description, title+trailing-blank, title+blank-lines+body) without false positives on titles that are substrings of unrelated words. The board-self-close suffix preserves the existing emoji-prefixed `📋 **Board Task ID**` audit anchor so downstream prompt-snapshot consumers continue to find it."

  :trace_references
    [".missiond/tasks/wave33/shared-memory.lisp :: wave33-01-claim-003"
     ".missiond/tasks/wave33/session-trace.lisp :: wave33-01-trace-preamble-read-004"])
