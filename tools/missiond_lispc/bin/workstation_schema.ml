open Ast

let rec collect_forms named node =
  let here = if is_list node named then [ node ] else [] in
  match node with
  | List (_, _, xs) -> here @ (xs |> List.map (collect_forms named) |> List.flatten)
  | _ -> here

let form_id = function
  | List (_, _, _ :: id_node :: _) -> atom_text id_node
  | _ -> None

let int_prop key props =
  match prop_text key props with
  | Some s -> (try Some (int_of_string s) with Failure _ -> None)
  | None -> None

let nonempty_text_prop key props =
  match prop_text key props with
  | Some value when String.trim value <> "" -> true
  | _ -> false

let prop_text_or_nil key props =
  match prop_text key props with
  | Some "nil" -> Some ""
  | other -> other

let has_form_id forms name id =
  forms
  |> List.exists (fun form -> is_list form name && form_id form = Some id)

let form_by_id forms name id =
  forms
  |> List.find_opt (fun form -> is_list form name && form_id form = Some id)

let list_prop_nonempty key props =
  match prop key props with
  | Some node -> list_texts node <> []
  | None -> false

let require_form diagnostics file root forms name id =
  if not (has_form_id forms name id) then
    diagnostics :=
      diag file (loc_of root)
        ("workstation." ^ name ^ "_missing")
        (Printf.sprintf "workstation-config missing (%s %s ...)" name id)
      :: !diagnostics

let require_prop diagnostics file form label key =
  let props = keyword_props ~start:2 form in
  if not (nonempty_text_prop key props || list_prop_nonempty key props) then
    diagnostics :=
      diag file (loc_of form) "workstation.prop_missing"
        (Printf.sprintf "%s missing non-empty %s" label key)
      :: !diagnostics

let validate_timeout_policy diagnostics file form id =
  let props = keyword_props ~start:2 form in
  let default = int_prop ":default_secs" props in
  let min = int_prop ":min_secs" props in
  let max = int_prop ":max_secs" props in
  (match (default, min, max) with
  | Some d, Some lo, Some hi when lo <= d && d <= hi -> ()
  | _ ->
      diagnostics :=
        diag file (loc_of form) "workstation.timeout_policy_invalid"
          (Printf.sprintf
             "timeout-policy %s must declare numeric :min_secs <= :default_secs <= :max_secs"
             id)
        :: !diagnostics);
  ()

let validate_model_profile diagnostics file forms id =
  match form_by_id forms "model-profile" id with
  | None ->
      diagnostics :=
        diag file { line = 1; column = 1 } "workstation.model_profile_missing"
          (Printf.sprintf "missing model-profile %s" id)
        :: !diagnostics
  | Some form ->
      let props = keyword_props ~start:2 form in
      if not (list_prop_nonempty ":applies-to" props) then
        diagnostics :=
          diag file (loc_of form) "workstation.model_profile_applies_to"
            (Printf.sprintf "model-profile %s must declare :applies-to" id)
          :: !diagnostics;
      if id = "coding-default-opus-4-7" then (
        if prop_text_or_nil ":spawn-model-arg" props <> Some "" then
          diagnostics :=
            diag file (loc_of form) "workstation.coding_default_model_override"
              "coding-default-opus-4-7 must omit CLI --model via :spawn-model-arg nil"
            :: !diagnostics;
        match prop_text ":effective-model" props with
        | Some value when contains_substring value "Opus 4.7" -> ()
        | _ ->
            diagnostics :=
              diag file (loc_of form) "workstation.coding_default_effective_model"
                "coding-default-opus-4-7 must document Opus 4.7 as the effective model"
              :: !diagnostics)

let validate_slot_template diagnostics file forms id =
  match form_by_id forms "slot-template" id with
  | None ->
      diagnostics :=
        diag file { line = 1; column = 1 } "workstation.slot_template_missing"
          (Printf.sprintf "missing slot-template %s" id)
        :: !diagnostics
  | Some form ->
      let props = keyword_props ~start:2 form in
      List.iter
        (require_prop diagnostics file form ("slot-template " ^ id))
        [ ":role"; ":description"; ":default-model-profile"; ":mcp-config"; ":default-cwd" ];
      (match prop_text ":mcp-config" props with
      | Some value
        when starts_with ~prefix:"/Users/" value
             && not (contains_substring value "$MISSION_HOME") ->
          diagnostics :=
            diag file (loc_of form) "workstation.host_absolute_mcp_config"
              (Printf.sprintf
                 "slot-template %s :mcp-config must be host-relative or use $MISSION_HOME, got %s"
                 id value)
            :: !diagnostics
      | _ -> ())

let validate_startup_slot diagnostics file form =
  let id = Option.value ~default:"<missing>" (form_id form) in
  List.iter
    (require_prop diagnostics file form ("startup-slot " ^ id))
    [ ":engine"; ":lifecycle"; ":slot_id"; ":role"; ":timeout_secs"; ":skip_permissions" ]

let validate_managed_node_policy diagnostics file forms =
  match form_by_id forms "managed-node-runtime-policy" "host-portability" with
  | None ->
      diagnostics :=
        diag file { line = 1; column = 1 } "workstation.managed_node_policy_missing"
          "missing (managed-node-runtime-policy host-portability ...)"
        :: !diagnostics
  | Some form ->
      let props = keyword_props ~start:2 form in
      if prop_text ":mcp-config-resolution" props <> Some "host-relative" then
        diagnostics :=
          diag file (loc_of form) "workstation.mcp_config_resolution"
            "managed-node-runtime-policy must declare :mcp-config-resolution host-relative"
          :: !diagnostics;
      if prop_text ":registered-project-roots-allowed" props <> Some "true" then
        diagnostics :=
          diag file (loc_of form) "workstation.project_root_cwd_policy"
            "managed-node-runtime-policy must allow registered project roots for dynamic slot cwd"
          :: !diagnostics;
      let required = [ "ttl_seconds"; "extend_count"; "message_count" ] in
      let present =
        match prop ":db-integer-portability" props with
        | Some node -> list_texts node
        | None -> []
      in
      List.iter
        (fun field ->
          if not (List.mem field present) then
            diagnostics :=
              diag file (loc_of form) "workstation.db_integer_portability"
                (Printf.sprintf
                   "managed-node-runtime-policy :db-integer-portability missing %s"
                   field)
              :: !diagnostics)
        required

let validate_provider_unavailable_policy diagnostics file forms =
  match form_by_id forms "pty-provider-unavailable-policy" "provider-blocked-diagnostics" with
  | None ->
      diagnostics :=
        diag file { line = 1; column = 1 } "workstation.pty_provider_policy_missing"
          "missing (pty-provider-unavailable-policy provider-blocked-diagnostics ...)"
        :: !diagnostics
  | Some form ->
      let states = collect_forms "state" form in
      List.iter
        (fun expected ->
          if not (has_form_id states "state" expected) then
            diagnostics :=
              diag file (loc_of form) "workstation.pty_provider_state_missing"
                (Printf.sprintf
                   "pty-provider-unavailable-policy missing state %s" expected)
              :: !diagnostics)
        [ "auth_missing"; "billing_or_account"; "usage_limit" ];
      states
      |> List.iter (fun state ->
             let id = Option.value ~default:"<missing>" (form_id state) in
             let props = keyword_props ~start:2 state in
             if prop_text ":state" props <> Some "blocked" then
               diagnostics :=
                 diag file (loc_of state) "workstation.pty_provider_state_not_blocked"
                   (Printf.sprintf "provider-unavailable state %s must project :state blocked" id)
                 :: !diagnostics;
             if not (list_prop_nonempty ":keywords" props) then
               diagnostics :=
                 diag file (loc_of state) "workstation.pty_provider_keywords"
                   (Printf.sprintf "provider-unavailable state %s must declare keywords" id)
                 :: !diagnostics)

let validate file =
  try
    let forms = Parser.parse_file file in
    let diagnostics = ref [] in
    let root = find_root forms "missiond-blueprint" in
    (match root with
    | None ->
        diagnostics :=
          diag file { line = 1; column = 1 } "root.missing"
            "missing missiond-blueprint root"
          :: !diagnostics
    | Some root -> (
        match find_child root "workstation-config" with
        | None ->
            diagnostics :=
              diag file (loc_of root) "workstation.config_missing"
                "missing workstation-config"
              :: !diagnostics
        | Some cfg ->
            let forms = children cfg in
            List.iter (validate_model_profile diagnostics file forms)
              [ "coding-default-opus-4-7"; "research-default"; "daily-sonnet" ];
            List.iter (validate_slot_template diagnostics file forms)
              [ "coder"; "researcher"; "ops" ];
            require_form diagnostics file cfg forms "cwd-policy" "dynamic-slot";
            require_form diagnostics file cfg forms "chat-completions-policy" "jarvis-api";
            List.iter
              (fun id ->
                match form_by_id forms "timeout-policy" id with
                | None -> require_form diagnostics file cfg forms "timeout-policy" id
                | Some form -> validate_timeout_policy diagnostics file form id)
              [ "boardtask-dispatch"; "claudecode-swarm"; "pty-send-blocking"; "dynamic-slot-spawn" ];
            (match form_by_id forms "ttl-policy" "dynamic-slot" with
            | Some form -> validate_timeout_policy diagnostics file form "ttl-policy dynamic-slot"
            | None -> require_form diagnostics file cfg forms "ttl-policy" "dynamic-slot");
            (match form_by_id forms "capacity-policy" "swarm-workers" with
            | None -> require_form diagnostics file cfg forms "capacity-policy" "swarm-workers"
            | Some form ->
                let props = keyword_props ~start:2 form in
                List.iter
                  (fun key ->
                    if int_prop key props = None then
                      diagnostics :=
                        diag file (loc_of form) "workstation.capacity_policy_field"
                          (Printf.sprintf "capacity-policy swarm-workers missing numeric %s" key)
                        :: !diagnostics)
                  [
                    ":default_claude_workers";
                    ":max_claude_workers";
                    ":default_gemini_workers";
                    ":max_gemini_workers";
                    ":dynamic_slot_limit";
                    ":delegate_rate_per_minute";
                  ]);
            forms
            |> List.filter (fun form -> is_list form "startup-slot")
            |> List.iter (validate_startup_slot diagnostics file);
            validate_managed_node_policy diagnostics file forms;
            validate_provider_unavailable_policy diagnostics file forms));
    (match root with
    | Some root -> (
        match find_child root "workstation-policy-shards" with
        | None ->
            diagnostics :=
              diag file (loc_of root) "workstation.policy_shards_missing"
                "missing workstation-policy-shards"
              :: !diagnostics
        | Some shards ->
            let policies = collect_forms "policy" shards |> List.filter_map form_id in
            List.iter
              (fun expected ->
                if not (List.mem expected policies) then
                  diagnostics :=
                    diag file (loc_of shards) "workstation.policy_shard_missing"
                      (Printf.sprintf "workstation-policy-shards missing %s" expected)
                    :: !diagnostics)
              [
                "slot-lifecycle-policy";
                "delegation-contract-policy";
                "completion-authority-policy";
                "cross-project-dispatch-policy";
                "context-prefetch-policy";
                "mcp-recovery-policy";
              ])
    | None -> ());
    List.rev !diagnostics
  with
  | Reader_error (l, msg) -> [ diag file l "parse.error" msg ]
  | Sys_error msg -> [ diag file { line = 1; column = 1 } "io.error" msg ]
