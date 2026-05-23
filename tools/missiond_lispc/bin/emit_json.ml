open Ast

let read_sorted_files dir suffix =
  Sys.readdir dir
  |> Array.to_list
  |> List.filter (fun name -> Filename.check_suffix name suffix)
  |> List.sort String.compare
  |> List.map (Filename.concat dir)

let compiled_envelope schema_version source_hash diagnostics payload =
  Printf.sprintf
    {|{"schema_version":%s,"source_hash":%s,"generated_at":null,"diagnostics":[%s],"payload":%s}|}
    (json_string schema_version)
    (json_string source_hash)
    (diagnostics |> List.map diagnostic_to_json |> String.concat ",")
    payload

let json_opt_string = function
  | Some value -> json_string value
  | None -> "null"

let json_string_list values =
  "[" ^ (values |> List.map json_string |> String.concat ",") ^ "]"

let list_forms named node =
  children node |> List.filter (fun child -> is_list child named)

let rec count_forms named node =
  let here = if is_list node named then 1 else 0 in
  match node with
  | List (_, _, xs) -> here + (xs |> List.map (count_forms named) |> List.fold_left ( + ) 0)
  | _ -> here

let rec collect_forms named node =
  let here = if is_list node named then [ node ] else [] in
  match node with
  | List (_, _, xs) -> here @ (xs |> List.map (collect_forms named) |> List.flatten)
  | _ -> here

let workflow_step_id = function
  | List (_, _, _ :: id_node :: _) -> atom_text id_node
  | _ -> None

let collect_workflow_step_ids props wf =
  match prop ":steps" props with
  | Some value ->
      let direct = list_texts value in
      if direct <> [] then direct
      else collect_forms "step" value |> List.filter_map workflow_step_id
  | None -> collect_forms "step" wf |> List.filter_map workflow_step_id

let count_named_or_list_entries named value =
  let named_count = count_forms named value in
  if named_count > 0 then named_count else List.length (list_texts value)

let prop_text_list key props =
  match prop key props with
  | Some value -> list_texts value
  | None -> []

let form_id = function
  | List (_, _, _ :: id_node :: _) -> atom_text id_node
  | _ -> None

let child_by_id parent name id =
  children parent
  |> List.find_opt (fun node -> is_list node name && form_id node = Some id)

let rec prop_text_any keys props =
  match keys with
  | [] -> None
  | key :: rest -> (
      match prop_text key props with
      | Some value -> Some value
      | None -> prop_text_any rest props)

let rec prop_any keys props =
  match keys with
  | [] -> None
  | key :: rest -> (
      match prop key props with
      | Some value -> Some value
      | None -> prop_any rest props)

let non_nil = function
  | Some "nil" -> None
  | Some value -> Some value
  | None -> None

let prop_opt_non_nil keys props = prop_text_any keys props |> non_nil

let json_field name value = Printf.sprintf "%s:%s" (json_string name) value

let json_assoc fields =
  "{"
  ^ (fields |> List.map (fun (name, value) -> json_field name value) |> String.concat ",")
  ^ "}"

let json_object_map entries =
  "{"
  ^ (entries |> List.map (fun (name, value) -> json_field name value) |> String.concat ",")
  ^ "}"

let json_array values = "[" ^ String.concat "," values ^ "]"

let json_bool value = if value then "true" else "false"

let json_bool_token ?(default = false) keys props =
  match prop_text_any keys props with
  | Some "true" -> "true"
  | Some "false" -> "false"
  | _ -> json_bool default

let json_number_token ?(default = "0") keys props =
  match prop_text_any keys props with
  | Some value -> value
  | None -> default

let json_string_token ?(default = "") keys props =
  json_string (Option.value ~default (prop_text_any keys props))

let json_opt_string_token keys props =
  json_opt_string (prop_opt_non_nil keys props)

let json_string_list_token keys props =
  match prop_any keys props with
  | Some value -> json_string_list (list_texts value)
  | None -> "[]"

let timeout_policy_json props =
  json_assoc
    [
      ("default_secs", json_number_token [ ":default_secs" ] props);
      ("min_secs", json_number_token [ ":min_secs" ] props);
      ("max_secs", json_number_token [ ":max_secs" ] props);
      ("watchdog_grace_secs", json_number_token [ ":watchdog_grace_secs" ] props);
      ( "missing_session_probe_secs",
        json_number_token [ ":missing_session_probe_secs" ] props );
    ]

let simple_timeout_policy_json props =
  json_assoc
    [
      ("default_secs", json_number_token [ ":default_secs" ] props);
      ("min_secs", json_number_token [ ":min_secs" ] props);
      ("max_secs", json_number_token [ ":max_secs" ] props);
    ]

let slot_ttl_policy_json props =
  json_assoc
    [
      ("default_secs", json_number_token [ ":default_secs" ] props);
      ("min_secs", json_number_token [ ":min_secs" ] props);
      ("max_secs", json_number_token [ ":max_secs" ] props);
      ("default_extend_secs", json_number_token [ ":default_extend_secs" ] props);
      ("max_extend_secs", json_number_token [ ":max_extend_secs" ] props);
    ]

let policy_props root name =
  Option.bind root (fun root -> find_child root name) |> Option.map (keyword_props ~start:1)

let named_child_props parent name id =
  child_by_id parent name id |> Option.map (keyword_props ~start:2)

let v3_surface_to_json node =
  let props = keyword_props ~start:2 node in
  let id =
    match children node with
    | _ :: id_node :: _ -> atom_text id_node
    | _ -> None
  in
  Printf.sprintf
    {|{"id":%s,"status":%s,"implements":%s,"code":%s}|}
    (json_opt_string id)
    (json_opt_string (prop_text ":status" props))
    (json_string_list (prop_text_list ":implements" props))
    (json_string_list (prop_text_list ":code" props))

let v3_function_to_json pillar_id node =
  let props = keyword_props ~start:2 node in
  let id =
    match children node with
    | _ :: id_node :: _ -> atom_text id_node
    | _ -> None
  in
  let steps =
    match prop ":core" props with
    | Some core -> collect_forms "step" core |> List.filter_map workflow_step_id
    | None -> []
  in
  Printf.sprintf
    {|{"pillar":%s,"id":%s,"surface":%s,"entry":%s,"egress":%s,"steps":%s}|}
    (json_string pillar_id)
    (json_opt_string id)
    (json_opt_string (prop_text ":surface" props))
    (json_string_list (prop_text_list ":entry" props))
    (json_string_list (prop_text_list ":egress" props))
    (json_string_list steps)

let v3_functions_to_json root =
  match find_child root "pillar-flow-map" with
  | None -> []
  | Some flow_map ->
      children flow_map
      |> List.filter (fun node -> is_list node "pillar")
      |> List.map (fun pillar ->
             let pillar_id =
               match children pillar with
               | _ :: id_node :: _ -> Option.value ~default:"<missing>" (atom_text id_node)
               | _ -> "<missing>"
             in
             children pillar
             |> List.filter (fun node -> is_list node "function")
             |> List.map (v3_function_to_json pillar_id))
      |> List.flatten

let v3_surfaces_to_json root =
  match find_child root "implementation-map" with
  | None -> []
  | Some implementation_map ->
      children implementation_map
      |> List.filter (fun node -> is_list node "surface")
      |> List.map v3_surface_to_json

let safe_id value =
  String.map
    (fun ch ->
      match ch with
      | 'a' .. 'z' | 'A' .. 'Z' | '0' .. '9' | '-' | '_' | '.' | ':' -> ch
      | _ -> '-')
    value

let source_map_json source_hash file node =
  let loc = loc_of node in
  let source_file = if loc.source_file = "" then file else loc.source_file in
  Printf.sprintf
    {|{"source_file":%s,"source_line":%d,"source_column":%d,"source_hash":%s}|}
    (json_string source_file) loc.line loc.column (json_string source_hash)

let semantic_function_fact source_hash file pillar_id node =
  let props = keyword_props ~start:2 node in
  let id =
    match children node with
    | _ :: id_node :: _ -> Option.value ~default:"<missing>" (atom_text id_node)
    | _ -> "<missing>"
  in
  let surface = prop_text ":surface" props in
  let entry = prop_text_list ":entry" props in
  let egress = prop_text_list ":egress" props in
  let steps =
    match prop ":core" props with
    | Some core -> collect_forms "step" core |> List.filter_map workflow_step_id
    | None -> []
  in
  Printf.sprintf
    {|{"fact_id":%s,"kind":"function","project_id":"missiond","pillar":%s,"id":%s,"surface":%s,"entry":%s,"core_steps":%s,"egress":%s,"source":%s}|}
    (json_string ("fn:" ^ safe_id pillar_id ^ ":" ^ safe_id id))
    (json_string pillar_id)
    (json_string id)
    (json_opt_string surface)
    (json_string_list entry)
    (json_string_list steps)
    (json_string_list egress)
    (source_map_json source_hash file node)

let semantic_surface_fact source_hash file node =
  let props = keyword_props ~start:2 node in
  let id =
    match children node with
    | _ :: id_node :: _ -> Option.value ~default:"<missing>" (atom_text id_node)
    | _ -> "<missing>"
  in
  Printf.sprintf
    {|{"fact_id":%s,"kind":"surface","project_id":"missiond","id":%s,"status":%s,"implements":%s,"code":%s,"source":%s}|}
    (json_string ("surface:" ^ safe_id id))
    (json_string id)
    (json_opt_string (prop_text ":status" props))
    (json_string_list (prop_text_list ":implements" props))
    (json_string_list (prop_text_list ":code" props))
    (source_map_json source_hash file node)

let semantic_artifact_fact source_hash file node =
  let props = keyword_props ~start:2 node in
  let id =
    match children node with
    | _ :: id_node :: _ -> Option.value ~default:"<missing>" (atom_text id_node)
    | _ -> "<missing>"
  in
  Printf.sprintf
    {|{"fact_id":%s,"kind":"artifact_contract","project_id":"missiond","id":%s,"schema":%s,"path":%s,"writer":%s,"ssot":%s,"required":%s,"source":%s}|}
    (json_string ("artifact:" ^ safe_id id))
    (json_string id)
    (json_opt_string (prop_text ":schema" props))
    (json_opt_string (prop_text ":path" props))
    (json_opt_string (prop_text ":writer" props))
    (json_bool_token [ ":ssot" ] props)
    (json_string_list (prop_text_list ":required" props))
    (source_map_json source_hash file node)

let semantic_workflow_contract_fact source_hash file node =
  let props = keyword_props ~start:2 node in
  let id =
    match children node with
    | _ :: id_node :: _ -> Option.value ~default:"<missing>" (atom_text id_node)
    | _ -> "<missing>"
  in
  Printf.sprintf
    {|{"fact_id":%s,"kind":"workflow_contract","project_id":"missiond","id":%s,"schema":%s,"path":%s,"writer":%s,"required":%s,"source":%s}|}
    (json_string ("workflow-contract:" ^ safe_id id))
    (json_string id)
    (json_opt_string (prop_text ":schema" props))
    (json_opt_string (prop_text ":path" props))
    (json_opt_string (prop_text ":writer" props))
    (json_string_list (prop_text_list ":required" props))
    (source_map_json source_hash file node)

let semantic_workstation_config_fact source_hash file node =
  let model_profiles =
    list_forms "model-profile" node |> List.filter_map form_id
  in
  let slot_templates =
    list_forms "slot-template" node |> List.filter_map form_id
  in
  Printf.sprintf
    {|{"fact_id":"workstation-config","kind":"workstation_config","project_id":"missiond","id":"workstation-config","model_profiles":%s,"slot_templates":%s,"source":%s}|}
    (json_string_list model_profiles)
    (json_string_list slot_templates)
    (source_map_json source_hash file node)

let semantic_source_unit_fact unit =
  Printf.sprintf
    {|{"fact_id":%s,"kind":"module_source_unit","project_id":"missiond","id":%s,"file":%s,"unit_kind":%s,"included_by":%s,"include_line":%s,"source_hash":%s}|}
    (json_string ("source-unit:" ^ safe_id unit.Source_resolver.file))
    (json_string unit.file)
    (json_string unit.file)
    (json_string unit.kind)
    (match unit.included_by with Some value -> json_string value | None -> "null")
    (match unit.include_line with Some value -> string_of_int value | None -> "null")
    (json_string unit.source_hash)

let runtime_policy_names =
  [
    "autopilot-policy";
    "cascade-policy";
    "flow-runtime-policy";
    "compute-runtime-policy";
    "minimax-runtime-policy";
    "router-runtime-policy";
    "project-registry-policy";
    "capability-governance-policy";
    "memory-kb-policy";
    "conversation-ingestion-policy";
    "learning-engine-policy";
  ]

let runtime_policy_payload_key = function
  | "autopilot-policy" -> "autopilot"
  | "cascade-policy" -> "cascade"
  | "flow-runtime-policy" -> "flow"
  | "compute-runtime-policy" -> "compute"
  | "minimax-runtime-policy" -> "minimax"
  | "router-runtime-policy" -> "router"
  | "project-registry-policy" -> "projectRegistry"
  | "capability-governance-policy" -> "capabilityGovernance"
  | "memory-kb-policy" -> "memoryKb"
  | "conversation-ingestion-policy" -> "conversationIngestion"
  | "learning-engine-policy" -> "learningEngine"
  | name -> name

let runtime_policy_descriptor_json source_hash file name node =
  let props = keyword_props ~start:1 node in
  let keyword_keys = props |> List.map fst in
  let nested_forms = children node |> List.filter_map head in
  json_assoc
    [
      ("id", json_string name);
      ("schema_version", json_string "missiond.runtime-policy-descriptor.v1");
      ("form", json_string name);
      ("payload_key", json_string (runtime_policy_payload_key name));
      ("keyword_keys", json_string_list keyword_keys);
      ("nested_forms", json_string_list nested_forms);
      ("source", source_map_json source_hash file node);
    ]

let runtime_policy_descriptors_json source_hash file root =
  runtime_policy_names
  |> List.filter_map (fun name ->
         find_child root name
         |> Option.map (runtime_policy_descriptor_json source_hash file name))
  |> json_array

let semantic_runtime_policy_fact source_hash file name node =
  let props = keyword_props ~start:1 node in
  let keyword_keys = props |> List.map fst in
  let nested_forms = children node |> List.filter_map head in
  Printf.sprintf
    {|{"fact_id":%s,"kind":"runtime_policy","project_id":"missiond","id":%s,"schema_version":"missiond.runtime-policy-descriptor.v1","form":%s,"payload_key":%s,"keyword_keys":%s,"nested_forms":%s,"source":%s}|}
    (json_string ("runtime-policy:" ^ safe_id name))
    (json_string name)
    (json_string name)
    (json_string (runtime_policy_payload_key name))
    (json_string_list keyword_keys)
    (json_string_list nested_forms)
    (source_map_json source_hash file node)

let checker_registry_json source_hash file root =
  match find_child root "compression-contract" with
  | None -> "[]"
  | Some node ->
      let props = keyword_props ~start:1 node in
      let checks =
        match prop ":checks" props with
        | Some value -> list_texts value
        | None -> []
      in
      [
        json_assoc
          [
            ("id", json_string "v3-compression-contract");
            ("checks", json_string_list checks);
            ("source", source_map_json source_hash file node);
          ];
      ]
      |> json_array

let semantic_checker_registry_fact source_hash file root =
  match find_child root "compression-contract" with
  | None -> []
  | Some node ->
      let props = keyword_props ~start:1 node in
      let checks =
        match prop ":checks" props with
        | Some value -> list_texts value
        | None -> []
      in
      [
        Printf.sprintf
          {|{"fact_id":"checker-registry:v3-compression-contract","kind":"checker_registry","project_id":"missiond","id":"v3-compression-contract","checks":%s,"source":%s}|}
          (json_string_list checks)
          (source_map_json source_hash file node);
      ]

let final_convergence_check_json node =
  let props = keyword_props ~start:2 node in
  let id = Option.value ~default:"<missing>" (form_id node) in
  let command =
    match prop_text ":command" props with Some value -> json_string value | None -> "null"
  in
  json_assoc
    [
      ("id", json_string id);
      ("command", command);
      ("argv", json_string_list_token [ ":argv" ] props);
      ("json", json_bool_token [ ":json" ] props);
      ("timeout_ms", json_number_token [ ":timeout-ms"; ":timeout_ms" ] props);
    ]

let final_convergence_needle_json node =
  let props = keyword_props ~start:2 node in
  let id = Option.value ~default:"<missing>" (form_id node) in
  json_assoc
    [
      ("id", json_string id);
      ("needle", json_string_token [ ":needle" ] props);
    ]

let final_convergence_facade_budget_json node =
  let props = keyword_props ~start:2 node in
  let id = Option.value ~default:"<missing>" (form_id node) in
  json_assoc
    [
      ("id", json_string id);
      ("file", json_string_token [ ":file" ] props);
      ("max_lines", json_number_token [ ":max-lines"; ":max_lines" ] props);
    ]

let final_convergence_runtime_file_json node =
  let props = keyword_props ~start:2 node in
  json_assoc
    [
      ("file", json_string_token [ ":file" ] props);
      ("needles", json_string_list_token [ ":needles" ] props);
    ]

let final_convergence_gate_fact source_hash file root =
  match find_child root "final-convergence-gate" with
  | None -> []
  | Some node ->
      let props = keyword_props ~start:1 node in
      let id = Option.value ~default:"v3-final-convergence" (prop_text ":id" props) in
      let live_checks =
        list_forms "live-check" node |> List.map final_convergence_check_json
      in
      let runtime_checks =
        list_forms "runtime-check" node |> List.map final_convergence_check_json
      in
      let blueprint_needles =
        list_forms "blueprint-needle" node
        |> List.map final_convergence_needle_json
      in
      let facade_budgets =
        list_forms "facade-budget" node
        |> List.map final_convergence_facade_budget_json
      in
      let required_split_files = prop_text_list ":required-split-files" props in
      let required_runtime_files =
        list_forms "runtime-file" node
        |> List.map final_convergence_runtime_file_json
      in
      [
        Printf.sprintf
          {|{"fact_id":%s,"kind":"final_convergence_gate","project_id":"missiond","id":%s,"live_checks":%s,"runtime_checks":%s,"blueprint_needles":%s,"facade_budgets":%s,"required_split_files":%s,"required_runtime_files":%s,"source":%s}|}
          (json_string ("final-convergence-gate:" ^ safe_id id))
          (json_string id)
          (json_array live_checks)
          (json_array runtime_checks)
          (json_array blueprint_needles)
          (json_array facade_budgets)
          (json_string_list required_split_files)
          (json_array required_runtime_files)
          (source_map_json source_hash file node);
      ]

let semantic_contract_split_facts source_hash file surface_node =
  let props = keyword_props ~start:2 surface_node in
  let surface_id = Option.value ~default:"<missing>" (form_id surface_node) in
  match prop ":contract-split" props with
  | None -> []
  | Some split ->
      children split
      |> List.filter_map (fun entry ->
             match head entry with
             | None -> None
             | Some id ->
                 let entry_props = keyword_props ~start:1 entry in
                 let owns =
                   match prop ":owns" entry_props with
                   | Some value -> list_texts value
                   | None -> []
                 in
                 Some
                   (Printf.sprintf
                      {|{"fact_id":%s,"kind":"contract_split","project_id":"missiond","surface":%s,"id":%s,"owns":%s,"source":%s}|}
                      (json_string
                         ("contract-split:" ^ safe_id surface_id ^ ":"
                        ^ safe_id id))
                      (json_string surface_id)
                      (json_string id)
                      (json_string_list owns)
                      (source_map_json source_hash file entry)))

let semantic_control_plane_domain_fact source_hash file node =
  let props = keyword_props ~start:2 node in
  let id = Option.value ~default:"<missing>" (form_id node) in
  Printf.sprintf
    {|{"fact_id":%s,"kind":"control_plane_domain","project_id":"missiond","id":%s,"owner":%s,"source_refs":%s,"functions":%s,"runtime_projection":%s,"checker":%s,"source":%s}|}
    (json_string ("control-plane-domain:" ^ safe_id id))
    (json_string id)
    (json_opt_string (prop_text ":owner" props))
    (json_string_list (prop_text_list ":source" props))
    (json_string_list (prop_text_list ":functions" props))
    (json_string_list (prop_text_list ":runtime-projection" props))
    (json_string_list (prop_text_list ":checker" props))
    (source_map_json source_hash file node)

let semantic_typed_subplane_facts source_hash file root =
  let contract_split_facts =
    match find_child root "implementation-map" with
    | None -> []
    | Some implementation_map ->
        children implementation_map
        |> List.filter (fun node -> is_list node "surface")
        |> List.map (semantic_contract_split_facts source_hash file)
        |> List.flatten
  in
  let control_plane_facts =
    match find_child root "control-plane-m6-split" with
    | None -> []
    | Some control_plane ->
        children control_plane
        |> List.filter (fun node -> is_list node "domain")
        |> List.map (semantic_control_plane_domain_fact source_hash file)
  in
  contract_split_facts @ control_plane_facts

let semantic_facts source_hash file source_units root =
  let function_facts =
    match find_child root "pillar-flow-map" with
    | None -> []
    | Some flow_map ->
        children flow_map
        |> List.filter (fun node -> is_list node "pillar")
        |> List.map (fun pillar ->
               let pillar_id =
                 match children pillar with
                 | _ :: id_node :: _ ->
                     Option.value ~default:"<missing>" (atom_text id_node)
                 | _ -> "<missing>"
               in
               children pillar
               |> List.filter (fun node -> is_list node "function")
               |> List.map (semantic_function_fact source_hash file pillar_id))
        |> List.flatten
  in
  let surface_facts =
    match find_child root "implementation-map" with
    | None -> []
    | Some implementation_map ->
        children implementation_map
        |> List.filter (fun node -> is_list node "surface")
        |> List.map (semantic_surface_fact source_hash file)
  in
  let artifact_facts =
    match find_child root "artifact-contracts" with
    | None -> []
    | Some contracts ->
        contracts
        |> list_forms "artifact"
        |> List.map (semantic_artifact_fact source_hash file)
  in
  let workflow_contract_facts =
    match find_child root "artifact-contracts" with
    | None -> []
    | Some contracts ->
        contracts
        |> list_forms "artifact"
        |> List.filter (fun node -> form_id node = Some "workflow")
        |> List.map (semantic_workflow_contract_fact source_hash file)
  in
  let workstation_facts =
    match find_child root "workstation-config" with
    | None -> []
    | Some workstation ->
        [ semantic_workstation_config_fact source_hash file workstation ]
  in
  let runtime_policy_facts =
    runtime_policy_names
    |> List.filter_map (fun name ->
           find_child root name
           |> Option.map (semantic_runtime_policy_fact source_hash file name))
  in
  let checker_registry_facts =
    semantic_checker_registry_fact source_hash file root
  in
  let final_convergence_gate_facts =
    final_convergence_gate_fact source_hash file root
  in
  let source_unit_facts =
    source_units |> List.map semantic_source_unit_fact
  in
  let typed_subplane_facts =
    semantic_typed_subplane_facts source_hash file root
  in
  function_facts @ surface_facts @ artifact_facts @ workflow_contract_facts
  @ workstation_facts @ runtime_policy_facts @ checker_registry_facts
  @ final_convergence_gate_facts @ source_unit_facts @ typed_subplane_facts

let project_entry_to_json node =
  let props = keyword_props ~start:1 node in
  let checks =
    match prop ":checks" props with
    | Some value -> list_texts value
    | None -> []
  in
  Printf.sprintf
    {|{"id":%s,"kind":%s,"root":%s,"path":%s,"intent":%s,"backend":%s,"frontend":%s,"operations":%s,"status":%s,"surface":%s,"checks":%s}|}
    (json_opt_string (prop_text ":id" props))
    (json_opt_string (prop_text ":kind" props))
    (json_opt_string (prop_text ":root" props))
    (json_opt_string (prop_text ":path" props))
    (json_opt_string (prop_text ":intent" props))
    (json_opt_string (prop_text ":backend" props))
    (json_opt_string (prop_text ":frontend" props))
    (json_opt_string (prop_text ":operations" props))
    (json_opt_string (prop_text ":status" props))
    (json_opt_string (prop_text ":surface" props))
    (json_string_list checks)

let maturity_entry_to_json node =
  let props = keyword_props ~start:1 node in
  let gap =
    match prop ":gap" props with
    | Some value -> list_texts value
    | None -> []
  in
  Printf.sprintf {|{"id":%s,"current":%s,"target":%s,"gap":%s}|}
    (json_opt_string (prop_text ":id" props))
    (json_opt_string (prop_text ":current" props))
    (json_opt_string (prop_text ":target" props))
    (json_string_list gap)

let hardening_entry_to_json node =
  let props = keyword_props ~start:1 node in
  let gap =
    match prop ":gap" props with
    | Some value -> list_texts value
    | None -> []
  in
  Printf.sprintf {|{"id":%s,"current":%s,"target":%s,"gap":%s}|}
    (json_opt_string (prop_text ":id" props))
    (json_opt_string (prop_text ":current" props))
    (json_opt_string (prop_text ":target" props))
    (json_string_list gap)

let workflow_entry_to_json file =
  let forms = Parser.parse_file file in
  match List.find_opt (fun n -> is_list n "workflow") forms with
  | None ->
      Printf.sprintf
        {|{"file":%s,"name":null,"workflow_id":null,"status":null,"source_plans":[],"steps":[],"risk_gate_count":0,"completion_criteria_count":0}|}
        (json_string file)
  | Some wf ->
      let props = keyword_props ~start:1 wf in
      let source_plans =
        match prop ":source_plans" props with
        | Some value -> list_texts value
        | None -> []
      in
      let steps =
        collect_workflow_step_ids props wf
      in
      let name =
        match children wf with
        | _ :: name_node :: _ -> atom_text name_node
        | _ -> None
      in
      let risk_gate_count =
        match prop ":risk-gates" props with
        | Some value -> count_named_or_list_entries "gate" value
        | None -> 0
      in
      let completion_criteria_count =
        match prop ":completion" props with
        | Some value -> count_named_or_list_entries "criterion" value
        | None -> 0
      in
      Printf.sprintf
        {|{"file":%s,"name":%s,"workflow_id":%s,"status":%s,"owner":%s,"authority":%s,"source_plans":%s,"steps":%s,"risk_gate_count":%d,"completion_criteria_count":%d}|}
        (json_string file)
        (json_opt_string name)
        (json_opt_string (prop_text ":workflow_id" props))
        (json_opt_string (prop_text ":status" props))
        (json_opt_string (prop_text ":owner" props))
        (json_opt_string (prop_text ":authority" props))
        (json_string_list source_plans)
        (json_string_list steps)
        risk_gate_count completion_criteria_count

let workstation_model_profile_spawn_args_json workstation =
  list_forms "model-profile" workstation
  |> List.filter_map (fun node ->
         match form_id node with
         | None -> None
         | Some id ->
             let props = keyword_props ~start:2 node in
             Some (id, json_opt_string_token [ ":spawn-model-arg" ] props))
  |> json_object_map

let workstation_slot_default_profiles_json workstation =
  list_forms "slot-template" workstation
  |> List.filter_map (fun node ->
         match form_id node with
         | None -> None
         | Some id -> (
             let props = keyword_props ~start:2 node in
             match prop_opt_non_nil [ ":default-model-profile" ] props with
             | Some profile -> Some (id, json_string profile)
             | None -> None))
  |> json_object_map

let workstation_slot_template_json node =
  let id = Option.value ~default:"<missing>" (form_id node) in
  let props = keyword_props ~start:2 node in
  json_assoc
    [
      ("name", json_string id);
      ("role", json_string_token [ ":role" ] props);
      ("description", json_string_token [ ":description" ] props);
      ("default_model_profile", json_opt_string_token [ ":default-model-profile" ] props);
      ("mcp_config", json_opt_string_token [ ":mcp-config" ] props);
      ("default_cwd", json_string_token [ ":default-cwd" ] props);
    ]

let workstation_slot_templates_json workstation =
  list_forms "slot-template" workstation
  |> List.filter_map (fun node ->
         form_id node |> Option.map (fun id -> (id, workstation_slot_template_json node)))
  |> json_object_map

let workstation_startup_slot_json node =
  let props = keyword_props ~start:2 node in
  json_assoc
    [
      ("task_type", json_string (Option.value ~default:"<missing>" (form_id node)));
      ("engine", json_string_token [ ":engine" ] props);
      ("lifecycle", json_string_token [ ":lifecycle" ] props);
      ("slot_id", json_opt_string_token [ ":slot_id"; ":slot-id" ] props);
      ("role", json_opt_string_token [ ":role" ] props);
      ("model_profile", json_opt_string_token [ ":model_profile"; ":model-profile" ] props);
      ("timeout_secs", json_number_token [ ":timeout_secs"; ":timeout-secs" ] props);
      ("skip_permissions", json_bool_token ~default:true [ ":skip_permissions"; ":skip-permissions" ] props);
    ]

let workstation_pool_worker_json node =
  let props = keyword_props ~start:2 node in
  json_assoc
    [
      ("id", json_string (Option.value ~default:"<missing>" (form_id node)));
      ("engine", json_string_token [ ":engine" ] props);
      ("role", json_string_token [ ":role" ] props);
      ("slot_id", json_string_token [ ":slot-id"; ":slot_id" ] props);
      ("task_type", json_string_token [ ":task-type"; ":task_type" ] props);
      ("model_profile", json_opt_string_token [ ":model-profile"; ":model_profile" ] props);
      ("model", json_opt_string_token [ ":model" ] props);
      ("task_classes", json_string_list_token [ ":task-classes"; ":task_classes" ] props);
      ("capabilities", json_string_list_token [ ":capabilities" ] props);
      ("max_concurrency", json_number_token ~default:"1" [ ":max-concurrency"; ":max_concurrency" ] props);
      ("timeout_secs", json_number_token ~default:"1800" [ ":timeout-secs"; ":timeout_secs" ] props);
      ("default_use", json_string_token [ ":default-use"; ":default_use" ] props);
      ("accepts_boardtask", json_bool_token ~default:true [ ":accepts-boardtask"; ":accepts_boardtask" ] props);
      ("write_allowed", json_bool_token ~default:false [ ":write-allowed"; ":write_allowed" ] props);
      ("reasoning_effort", json_opt_string_token [ ":reasoning-effort"; ":reasoning_effort" ] props);
      ("search_enabled", json_bool_token ~default:false [ ":search"; ":search-enabled"; ":search_enabled" ] props);
      ("sandbox", json_opt_string_token [ ":sandbox" ] props);
      ("approval_policy", json_opt_string_token [ ":approval-policy"; ":approval_policy" ] props);
      ("tool_policy_path", json_opt_string_token [ ":tool-policy-path"; ":tool_policy_path" ] props);
    ]

let workstation_pool_json root =
  match Option.bind root (fun root -> find_child root "workstation-pool") with
  | Some pool ->
      pool |> list_forms "worker" |> List.map workstation_pool_worker_json |> json_array
  | None -> "[]"

let workstation_runtime_config_json root =
  match Option.bind root (fun root -> find_child root "workstation-config") with
  | None -> "{}"
  | Some workstation ->
      let cwd_policy = named_child_props workstation "cwd-policy" "dynamic-slot" in
      let chat_policy =
        named_child_props workstation "chat-completions-policy" "jarvis-api"
      in
      let boardtask_timeout =
        named_child_props workstation "timeout-policy" "boardtask-dispatch"
      in
      let cc_swarm_timeout =
        named_child_props workstation "timeout-policy" "claudecode-swarm"
      in
      let pty_send_timeout =
        named_child_props workstation "timeout-policy" "pty-send-blocking"
      in
      let dynamic_slot_spawn_timeout =
        named_child_props workstation "timeout-policy" "dynamic-slot-spawn"
      in
      let context_pack_dispatch =
        named_child_props workstation "dispatch-policy" "context-pack-run-wave"
      in
      let capacity = named_child_props workstation "capacity-policy" "swarm-workers" in
      let ttl = named_child_props workstation "ttl-policy" "dynamic-slot" in
      json_assoc
        [
          ("slot_default_profiles", workstation_slot_default_profiles_json workstation);
          ("slot_templates", workstation_slot_templates_json workstation);
          ("model_profile_spawn_args", workstation_model_profile_spawn_args_json workstation);
          ( "startup_slots",
            workstation
            |> list_forms "startup-slot"
            |> List.map workstation_startup_slot_json
            |> json_array );
          ("workstation_pool", workstation_pool_json root);
          ( "allowed_cwd_prefixes",
            cwd_policy
            |> Option.map (json_string_list_token [ ":allowed-prefixes" ])
            |> Option.value ~default:"[]" );
          ( "chat_completions_default_slot",
            chat_policy
            |> Option.map (fun props ->
                   json_string_token [ ":default_slot"; ":default-slot" ] props)
            |> Option.value ~default:(json_string "") );
          ( "timeout_policy",
            boardtask_timeout
            |> Option.map timeout_policy_json
            |> Option.value ~default:"{}" );
          ( "cc_swarm_timeout_policy",
            cc_swarm_timeout
            |> Option.map simple_timeout_policy_json
            |> Option.value ~default:"{}" );
          ( "pty_send_timeout_policy",
            pty_send_timeout
            |> Option.map simple_timeout_policy_json
            |> Option.value ~default:"{}" );
          ( "dynamic_slot_spawn_timeout_policy",
            dynamic_slot_spawn_timeout
            |> Option.map simple_timeout_policy_json
            |> Option.value ~default:"{}" );
          ( "context_pack_dispatch_policy",
            context_pack_dispatch
            |> Option.map (fun props ->
                   json_assoc
                     [
                       ("default_max_parallel", json_number_token [ ":default_max_parallel" ] props);
                       ("min_parallel", json_number_token [ ":min_parallel" ] props);
                       ("max_parallel", json_number_token [ ":max_parallel" ] props);
                     ])
            |> Option.value ~default:"{}" );
          ( "swarm_capacity_policy",
            capacity
            |> Option.map (fun props ->
                   json_assoc
                     [
                       ("default_claude_workers", json_number_token [ ":default_claude_workers" ] props);
                       ("max_claude_workers", json_number_token [ ":max_claude_workers" ] props);
                       ("default_gemini_workers", json_number_token [ ":default_gemini_workers" ] props);
                       ("max_gemini_workers", json_number_token [ ":max_gemini_workers" ] props);
                       ("dynamic_slot_limit", json_number_token [ ":dynamic_slot_limit" ] props);
                       ("delegate_rate_per_minute", json_number_token [ ":delegate_rate_per_minute" ] props);
                     ])
            |> Option.value ~default:"{}" );
          ("slot_ttl_policy", ttl |> Option.map slot_ttl_policy_json |> Option.value ~default:"{}");
        ]

let flow_runtime_config_json root =
  match policy_props root "flow-runtime-policy" with
  | None -> "{}"
  | Some props ->
      json_assoc
        [
          ("llm_call_default_max_tokens", json_number_token [ ":llm-call-default-max-tokens" ] props);
          ("slot_task_default_model", json_string_token [ ":slot-task-default-model" ] props);
          ("slot_task_default_timeout_secs", json_number_token [ ":slot-task-default-timeout-secs" ] props);
          ("parallel_slot_default_parallelism", json_number_token [ ":parallel-slot-default-parallelism" ] props);
          ("parallel_slot_default_timeout_secs", json_number_token [ ":parallel-slot-default-timeout-secs" ] props);
        ]

let compute_runtime_config_json root =
  match Option.bind root (fun root -> find_child root "compute-runtime-policy") with
  | None -> "{}"
  | Some compute -> (
      match named_child_props compute "timeout-policy" "tracked-pty-spawn" with
      | None -> "{}"
      | Some props ->
          json_assoc [ ("pty_spawn_timeout_policy", simple_timeout_policy_json props) ])

let minimax_runtime_config_json root =
  match policy_props root "minimax-runtime-policy" with
  | None -> "{}"
  | Some props ->
      json_assoc
        [
          ("model", json_string_token [ ":model" ] props);
          ("direct_http_timeout_secs", json_number_token [ ":direct-http-timeout-secs" ] props);
          ("quota_throttle_secs", json_number_token [ ":quota-throttle-secs" ] props);
          ("default_max_tokens", json_number_token [ ":default-max-tokens" ] props);
        ]

let router_runtime_config_json root =
  match policy_props root "router-runtime-policy" with
  | None -> "{}"
  | Some props ->
      json_assoc
        [
          ("default_chat_model", json_string_token [ ":default-chat-model" ] props);
          ("chat_default_max_tokens", json_number_token [ ":chat-default-max-tokens" ] props);
          ("file_chat_default_max_tokens", json_number_token [ ":file-chat-default-max-tokens" ] props);
          ("flow_gemini_model", json_string_token [ ":flow-gemini-model" ] props);
          ("stateless_sonnet_model", json_string_token [ ":stateless-sonnet-model" ] props);
          ("queued_sonnet_model", json_string_token [ ":queued-sonnet-model" ] props);
          ("anthropic_urgent_model", json_string_token [ ":anthropic-urgent-model" ] props);
          ("anthropic_ops_model", json_string_token [ ":anthropic-ops-model" ] props);
          ("anthropic_docs_test_chore_model", json_string_token [ ":anthropic-docs-test-chore-model" ] props);
          ("compress_model", json_string_token [ ":compress-model" ] props);
          ("compress_channel", json_string_token [ ":compress-channel" ] props);
          ("compress_max_tokens", json_number_token [ ":compress-max-tokens" ] props);
          ("compress_char_budget_chars", json_number_token [ ":compress-char-budget-chars" ] props);
          ("direct_http_timeout_secs", json_number_token [ ":direct-http-timeout-secs" ] props);
          ("router_chat_idle_timeout_secs", json_number_token [ ":router-chat-idle-timeout-secs" ] props);
          ("router_chat_retry_max_attempts", json_number_token [ ":router-chat-retry-max-attempts" ] props);
          ("router_chat_retry_initial_backoff_ms", json_number_token [ ":router-chat-retry-initial-backoff-ms" ] props);
          ("router_chat_retry_max_backoff_ms", json_number_token [ ":router-chat-retry-max-backoff-ms" ] props);
          ("gemini_pty_queue_timeout_secs", json_number_token [ ":gemini-pty-queue-timeout-secs" ] props);
          ("gemini_http_queue_timeout_secs", json_number_token [ ":gemini-http-queue-timeout-secs" ] props);
          ("gemini_file_upload_timeout_secs", json_number_token [ ":gemini-file-upload-timeout-secs" ] props);
          ("gemini_file_poll_timeout_secs", json_number_token [ ":gemini-file-poll-timeout-secs" ] props);
          ("gemini_cli_absolute_timeout_secs", json_number_token [ ":gemini-cli-absolute-timeout-secs" ] props);
          ("gemini_cli_tool_exec_timeout_secs", json_number_token [ ":gemini-cli-tool-exec-timeout-secs" ] props);
          ("queued_sonnet_quota_throttle_secs", json_number_token [ ":queued-sonnet-quota-throttle-secs" ] props);
          ("queued_sonnet_default_max_tokens", json_number_token [ ":queued-sonnet-default-max-tokens" ] props);
        ]

let cascade_runtime_config_json root =
  match policy_props root "cascade-policy" with
  | None -> "{}"
  | Some props ->
      json_assoc
        [
          ("default_manifest_path", json_string_token [ ":default-manifest" ] props);
          ("allowed_root", json_string_token [ ":allowed-root" ] props);
          ("trigger_enabled", json_bool_token [ ":trigger-enabled" ] props);
          ("default_max_cycles", json_number_token [ ":default-max-cycles" ] props);
          ("max_cycles_limit", json_number_token [ ":max-cycles-limit" ] props);
        ]

let project_registry_runtime_config_json root =
  match policy_props root "project-registry-policy" with
  | None -> "{}"
  | Some props ->
      json_assoc
        [
          ("intent_path_candidates", json_string_list_token [ ":intent-path-candidates" ] props);
          ("default_universe_manifest", json_string_token [ ":default-universe-manifest" ] props);
        ]

let capability_governance_runtime_config_json root =
  match policy_props root "capability-governance-policy" with
  | None -> "{}"
  | Some props ->
      json_assoc
        [
          ("review_sidecar_path", json_string_token [ ":review-sidecar" ] props);
          ("protected_tool_patterns", json_string_list_token [ ":protected-tool-patterns" ] props);
          ("protected_flow_patterns", json_string_list_token [ ":protected-flow-patterns" ] props);
        ]

let memory_kb_runtime_config_json root =
  match policy_props root "memory-kb-policy" with
  | None -> "{}"
  | Some props ->
      json_assoc
        [
          ("pending_message_limit", json_number_token [ ":pending-message-limit" ] props);
          ("tool_result_preview_chars", json_number_token [ ":tool-result-preview-chars" ] props);
          ("assistant_preview_chars", json_number_token [ ":assistant-preview-chars" ] props);
        ]

let conversation_ingestion_runtime_config_json root =
  match policy_props root "conversation-ingestion-policy" with
  | None -> "{}"
  | Some props ->
      json_assoc
        [
          ("conversation_get_tail_default", json_number_token [ ":conversation-get-tail-default" ] props);
          ("conversation_search_default_limit", json_number_token [ ":conversation-search-default-limit" ] props);
          ("message_search_default_limit", json_number_token [ ":message-search-default-limit" ] props);
          ("context_before_default", json_number_token [ ":context-before-default" ] props);
          ("context_after_default", json_number_token [ ":context-after-default" ] props);
          ("conversation_events_default_limit", json_number_token [ ":conversation-events-default-limit" ] props);
          ("agent_trajectory_default_limit", json_number_token [ ":agent-trajectory-default-limit" ] props);
          ("timeline_query_default_limit", json_number_token [ ":timeline-query-default-limit" ] props);
          ("timeline_query_max_limit", json_number_token [ ":timeline-query-max-limit" ] props);
          ("timeline_search_default_limit", json_number_token [ ":timeline-search-default-limit" ] props);
          ("timeline_search_max_limit", json_number_token [ ":timeline-search-max-limit" ] props);
          ("intent_router_model", json_string_token [ ":intent-router-model" ] props);
          ("intent_router_timeout_ms", json_number_token [ ":intent-router-timeout-ms" ] props);
          ("vision_codex_binary", json_string_token [ ":vision-codex-binary" ] props);
          ("vision_codex_model", json_string_token [ ":vision-codex-model" ] props);
          ("vision_codex_idle_timeout_secs", json_number_token [ ":vision-codex-idle-timeout-secs" ] props);
          ("vision_codex_absolute_timeout_secs", json_number_token [ ":vision-codex-absolute-timeout-secs" ] props);
        ]

let autopilot_runtime_config_json root =
  let boardtask_timeout =
    Option.bind root (fun root -> find_child root "workstation-config")
    |> fun workstation ->
    Option.bind workstation (fun workstation ->
        named_child_props workstation "timeout-policy" "boardtask-dispatch")
  in
  match policy_props root "autopilot-policy" with
  | None -> "{}"
  | Some props ->
      json_assoc
        [
          ("boardtask_timeout_policy", boardtask_timeout |> Option.map timeout_policy_json |> Option.value ~default:"{}");
          ("stale_conversation_minutes", json_number_token [ ":stale-conversation-minutes" ] props);
          ("slot_task_reap_stale_secs", json_number_token [ ":slot-task-reap-stale-secs" ] props);
          ("recover_stale_running_minutes", json_number_token [ ":recover-stale-running-minutes" ] props);
          ("slot_failure_throttle_secs", json_number_token [ ":slot-failure-throttle-secs" ] props);
          ("deploy_review_timeout_secs", json_number_token [ ":deploy-review-timeout-secs" ] props);
          ("dynamic_slot_expiring_soon_secs", json_number_token [ ":dynamic-slot-expiring-soon-secs" ] props);
          ("stale_board_progress_minutes", json_number_token [ ":stale-board-progress-minutes" ] props);
          ("completed_job_gc_minutes", json_number_token [ ":completed-job-gc-minutes" ] props);
          ("idle_persistent_slot_secs", json_number_token [ ":idle-persistent-slot-secs" ] props);
          ("recent_intents_window_secs", json_number_token [ ":recent-intents-window-secs" ] props);
          ("user_stuck_cooldown_secs", json_number_token [ ":user-stuck-cooldown-secs" ] props);
          ("direction_shift_cooldown_secs", json_number_token [ ":direction-shift-cooldown-secs" ] props);
        ]

let learning_engine_runtime_config_json root =
  match policy_props root "learning-engine-policy" with
  | None -> "{}"
  | Some props ->
      json_assoc
        [
          ("realtime_extraction_timeout_secs", json_number_token [ ":realtime-extraction-timeout-secs" ] props);
          ("realtime_empty_backoff_base_secs", json_number_token [ ":realtime-empty-backoff-base-secs" ] props);
          ("realtime_empty_backoff_max_secs", json_number_token [ ":realtime-empty-backoff-max-secs" ] props);
          ("deep_analysis_zero_output_fuse_threshold", json_number_token [ ":deep-analysis-zero-output-fuse-threshold" ] props);
          ("deep_analysis_zero_output_fuse_secs", json_number_token [ ":deep-analysis-zero-output-fuse-secs" ] props);
          ("decision_tier3_timeout_secs", json_number_token [ ":decision-tier3-timeout-secs" ] props);
          ("habit_scan_timeout_secs", json_number_token [ ":habit-scan-timeout-secs" ] props);
          ("token_spend_guard_window_secs", json_number_token [ ":token-spend-guard-window-secs" ] props);
          ("token_spend_guard_soft_limit", json_number_token [ ":token-spend-guard-soft-limit" ] props);
          ("timeline_analysis_interval_secs", json_number_token [ ":timeline-analysis-interval-secs" ] props);
          ("timeline_analysis_window_hours", json_number_token [ ":timeline-analysis-window-hours" ] props);
          ("timeline_error_limit", json_number_token [ ":timeline-error-limit" ] props);
          ("timeline_llm_sample_limit", json_number_token [ ":timeline-llm-sample-limit" ] props);
          ("timeline_slow_event_limit", json_number_token [ ":timeline-slow-event-limit" ] props);
          ("timeline_slow_threshold_ms", json_number_token [ ":timeline-slow-threshold-ms" ] props);
          ("idle_explore_interval_secs", json_number_token [ ":idle-explore-interval-secs" ] props);
          ("habit_scan_interval_secs", json_number_token [ ":habit-scan-interval-secs" ] props);
          ("habit_scan_batch_size", json_number_token [ ":habit-scan-batch-size" ] props);
          ("kb_auto_gc_interval_secs", json_number_token [ ":kb-auto-gc-interval-secs" ] props);
          ("kb_consolidation_interval_secs", json_number_token [ ":kb-consolidation-interval-secs" ] props);
          ("kb_reflection_interval_secs", json_number_token [ ":kb-reflection-interval-secs" ] props);
          ("kb_reflection_utility_threshold", json_number_token [ ":kb-reflection-utility-threshold" ] props);
          ("kb_reflection_min_access", json_number_token [ ":kb-reflection-min-access" ] props);
          ("kb_reflection_max_entries", json_number_token [ ":kb-reflection-max-entries" ] props);
          ("kb_reflection_max_tokens", json_number_token [ ":kb-reflection-max-tokens" ] props);
          ("decision_harvest_interval_secs", json_number_token [ ":decision-harvest-interval-secs" ] props);
          ("cooccurrence_refresh_interval_secs", json_number_token [ ":cooccurrence-refresh-interval-secs" ] props);
        ]

let runtime_config_payload_json blueprint source_hash source_units root =
  json_assoc
    [
      ("blueprint", json_string blueprint);
      ("source_units", Source_resolver.source_units_to_json source_units);
      ( "runtime_policies",
        root
        |> Option.map (runtime_policy_descriptors_json source_hash blueprint)
        |> Option.value ~default:"[]" );
      ("workstation", workstation_runtime_config_json root);
      ("flow", flow_runtime_config_json root);
      ("compute", compute_runtime_config_json root);
      ("minimax", minimax_runtime_config_json root);
      ("router", router_runtime_config_json root);
      ("cascade", cascade_runtime_config_json root);
      ("projectRegistry", project_registry_runtime_config_json root);
      ("capabilityGovernance", capability_governance_runtime_config_json root);
      ("memoryKb", memory_kb_runtime_config_json root);
      ("conversationIngestion", conversation_ingestion_runtime_config_json root);
      ("autopilot", autopilot_runtime_config_json root);
      ("learningEngine", learning_engine_runtime_config_json root);
    ]

let require_props diagnostics file node label keys =
  let props = keyword_props ~start:1 node in
  List.iter
    (fun key ->
      if prop key props = None then
        diagnostics :=
          diag ~path:label file (loc_of node) "runtime_config.prop_missing"
            (Printf.sprintf "%s missing %s" label key)
          :: !diagnostics)
    keys

let require_policy diagnostics file root name keys =
  match find_child root name with
  | None ->
      diagnostics :=
        diag ~path:name file (loc_of root) "runtime_config.policy_missing"
          (Printf.sprintf "missing (%s ...)" name)
        :: !diagnostics
  | Some node -> require_props diagnostics file node name keys

let require_named_policy diagnostics file parent parent_label name id keys =
  match child_by_id parent name id with
  | None ->
      diagnostics :=
        diag ~path:parent_label file (loc_of parent) "runtime_config.policy_missing"
          (Printf.sprintf "missing (%s %s ...) in %s" name id parent_label)
        :: !diagnostics
  | Some node ->
      let props = keyword_props ~start:2 node in
      List.iter
        (fun key ->
          if prop key props = None then
            diagnostics :=
              diag ~path:(parent_label ^ "." ^ id) file (loc_of node)
                "runtime_config.prop_missing"
                (Printf.sprintf "%s %s missing %s" name id key)
              :: !diagnostics)
        keys

let runtime_config_required_diagnostics file root =
  let diagnostics = ref [] in
  (match find_child root "workstation-pool" with
  | None ->
      diagnostics :=
        diag ~path:"workstation-pool" file (loc_of root)
          "runtime_config.policy_missing" "missing (workstation-pool ...)"
        :: !diagnostics
  | Some pool ->
      if list_forms "worker" pool = [] then
        diagnostics :=
          diag ~path:"workstation-pool" file (loc_of pool)
            "runtime_config.worker_missing"
            "workstation-pool must declare at least one worker"
          :: !diagnostics);
  (match find_child root "workstation-config" with
  | None -> ()
  | Some workstation ->
      require_named_policy diagnostics file workstation "workstation-config"
        "dispatch-policy" "context-pack-run-wave"
        [ ":default_max_parallel"; ":min_parallel"; ":max_parallel" ]);
  require_policy diagnostics file root "autopilot-policy"
    [
      ":stale-conversation-minutes";
      ":slot-task-reap-stale-secs";
      ":recover-stale-running-minutes";
      ":slot-failure-throttle-secs";
      ":deploy-review-timeout-secs";
      ":dynamic-slot-expiring-soon-secs";
      ":stale-board-progress-minutes";
      ":completed-job-gc-minutes";
      ":idle-persistent-slot-secs";
      ":recent-intents-window-secs";
      ":user-stuck-cooldown-secs";
      ":direction-shift-cooldown-secs";
    ];
  require_policy diagnostics file root "cascade-policy"
    [
      ":default-manifest";
      ":allowed-root";
      ":trigger-enabled";
      ":default-max-cycles";
      ":max-cycles-limit";
    ];
  require_policy diagnostics file root "flow-runtime-policy"
    [
      ":llm-call-default-max-tokens";
      ":slot-task-default-model";
      ":slot-task-default-timeout-secs";
      ":parallel-slot-default-parallelism";
      ":parallel-slot-default-timeout-secs";
    ];
  (match find_child root "compute-runtime-policy" with
  | None ->
      diagnostics :=
        diag ~path:"compute-runtime-policy" file (loc_of root)
          "runtime_config.policy_missing" "missing (compute-runtime-policy ...)"
        :: !diagnostics
  | Some compute ->
      require_named_policy diagnostics file compute "compute-runtime-policy"
        "timeout-policy" "tracked-pty-spawn"
        [ ":default_secs"; ":min_secs"; ":max_secs" ]);
  require_policy diagnostics file root "minimax-runtime-policy"
    [ ":model"; ":direct-http-timeout-secs"; ":quota-throttle-secs"; ":default-max-tokens" ];
  require_policy diagnostics file root "router-runtime-policy"
    [
      ":default-chat-model";
      ":chat-default-max-tokens";
      ":file-chat-default-max-tokens";
      ":flow-gemini-model";
      ":stateless-sonnet-model";
      ":queued-sonnet-model";
      ":anthropic-urgent-model";
      ":anthropic-ops-model";
      ":anthropic-docs-test-chore-model";
      ":compress-model";
      ":compress-channel";
      ":compress-max-tokens";
      ":compress-char-budget-chars";
      ":direct-http-timeout-secs";
      ":router-chat-idle-timeout-secs";
      ":router-chat-retry-max-attempts";
      ":router-chat-retry-initial-backoff-ms";
      ":router-chat-retry-max-backoff-ms";
      ":gemini-pty-queue-timeout-secs";
      ":gemini-http-queue-timeout-secs";
      ":gemini-file-upload-timeout-secs";
      ":gemini-file-poll-timeout-secs";
      ":gemini-cli-absolute-timeout-secs";
      ":gemini-cli-tool-exec-timeout-secs";
      ":queued-sonnet-quota-throttle-secs";
      ":queued-sonnet-default-max-tokens";
    ];
  require_policy diagnostics file root "project-registry-policy"
    [ ":intent-path-candidates"; ":default-universe-manifest" ];
  require_policy diagnostics file root "capability-governance-policy"
    [ ":review-sidecar"; ":protected-tool-patterns"; ":protected-flow-patterns" ];
  require_policy diagnostics file root "memory-kb-policy"
    [
      ":pending-message-limit";
      ":tool-result-preview-chars";
      ":assistant-preview-chars";
    ];
  require_policy diagnostics file root "learning-engine-policy"
    [
      ":realtime-extraction-timeout-secs";
      ":realtime-empty-backoff-base-secs";
      ":realtime-empty-backoff-max-secs";
      ":deep-analysis-zero-output-fuse-threshold";
      ":deep-analysis-zero-output-fuse-secs";
      ":decision-tier3-timeout-secs";
      ":habit-scan-timeout-secs";
      ":token-spend-guard-window-secs";
      ":token-spend-guard-soft-limit";
      ":timeline-analysis-interval-secs";
      ":timeline-analysis-window-hours";
      ":timeline-error-limit";
      ":timeline-llm-sample-limit";
      ":timeline-slow-event-limit";
      ":timeline-slow-threshold-ms";
      ":idle-explore-interval-secs";
      ":habit-scan-interval-secs";
      ":habit-scan-batch-size";
      ":kb-auto-gc-interval-secs";
      ":kb-consolidation-interval-secs";
      ":kb-reflection-interval-secs";
      ":kb-reflection-utility-threshold";
      ":kb-reflection-min-access";
      ":kb-reflection-max-entries";
      ":kb-reflection-max-tokens";
      ":decision-harvest-interval-secs";
      ":cooccurrence-refresh-interval-secs";
    ];
  require_policy diagnostics file root "conversation-ingestion-policy"
    [
      ":conversation-get-tail-default";
      ":conversation-search-default-limit";
      ":message-search-default-limit";
      ":context-before-default";
      ":context-after-default";
      ":conversation-events-default-limit";
      ":agent-trajectory-default-limit";
      ":timeline-query-default-limit";
      ":timeline-query-max-limit";
      ":timeline-search-default-limit";
      ":timeline-search-max-limit";
      ":intent-router-model";
      ":intent-router-timeout-ms";
      ":vision-codex-binary";
      ":vision-codex-model";
      ":vision-codex-idle-timeout-secs";
      ":vision-codex-absolute-timeout-secs";
    ];
  List.rev !diagnostics

let contract_abi_payload_json blueprint source_hash source_units root =
  let surfaces =
    root |> Option.map v3_surfaces_to_json |> Option.value ~default:[]
  in
  let functions =
    root |> Option.map v3_functions_to_json |> Option.value ~default:[]
  in
  let facts =
    root
    |> Option.map (semantic_facts source_hash blueprint source_units)
    |> Option.value ~default:[]
  in
  json_assoc
    [
      ("blueprint", json_string blueprint);
      ("source_units", Source_resolver.source_units_to_json source_units);
      ("surfaces", json_array surfaces);
      ("functions", json_array functions);
      ("facts", json_array facts);
      ( "runtime_policies",
        root
        |> Option.map (runtime_policy_descriptors_json source_hash blueprint)
        |> Option.value ~default:"[]" );
      ( "checker_registry",
        root
        |> Option.map (checker_registry_json source_hash blueprint)
        |> Option.value ~default:"[]" );
      ( "plan_contract",
        json_assoc
          [
            ("schema_version", json_string "missiond.plan-contract.v1");
            ("accepted_heads", json_string_list [ "plan"; "plan-draft" ]);
            ( "top_level_hint_keys",
              json_string_list
                [
                  ":target";
                  ":flow-id";
                  ":dispatch-strategy";
                  ":parallelism";
                  ":target-project";
                  ":requested-cwd";
                  ":objective";
                  ":summary";
                  ":scope";
                  ":commit-policy";
                  ":owned-files";
                  ":forbidden-files";
                  ":acceptance-commands";
                  ":workstation-dispatch";
                ] );
            ( "node_hint_keys",
              json_string_list
                [
                  ":id";
                  ":target";
                  ":depends-on";
                  ":workstation-dispatch";
                  ":acceptance";
                  ":rollback";
                  ":max-attempts";
                  ":retry-count";
                  ":timeout-ms";
                ] );
          ] );
    ]

let normalize_keyword key =
  let s =
    if starts_with ~prefix:":" key then
      String.sub key 1 (String.length key - 1)
    else key
  in
  String.map (function '-' -> '_' | c -> c) s

let rec plan_value_to_json = function
  | Atom (_, "nil") -> "null"
  | Atom (_, "true") -> "true"
  | Atom (_, "false") -> "false"
  | Atom (_, value) -> json_string value
  | String (_, value) -> json_string value
  | List (_, _, xs) -> xs |> List.map plan_value_to_json |> json_array

let props_to_object_json ?keys props =
  let key_allowed key =
    match keys with
    | None -> true
    | Some keys -> List.mem key keys
  in
  props
  |> List.filter_map (fun (key, value) ->
         if not (key_allowed key) then None
         else value |> Option.map (fun value -> (normalize_keyword key, plan_value_to_json value)))
  |> json_object_map

let plan_hint_keys =
  [
    ":target";
    ":flow-id";
    ":dispatch-strategy";
    ":parallelism";
    ":target-project";
    ":requested-cwd";
    ":objective";
    ":summary";
    ":scope";
    ":commit-policy";
    ":owned-files";
    ":forbidden-files";
    ":acceptance-commands";
    ":workstation-dispatch";
  ]

let plan_node_contract_json node =
  let props = keyword_props ~start:1 node in
  let depends_on =
    match prop ":depends-on" props with
    | Some value -> list_texts value
    | None -> []
  in
  json_assoc
    [
      ("id", json_opt_string (prop_text ":id" props));
      ("target", json_opt_string (prop_text ":target" props));
      ("depends_on", json_string_list depends_on);
      ("hints", props_to_object_json props);
      ("source", source_map_json "" "" node);
    ]

let plan_contract_payload_json file forms =
  let root =
    forms
    |> List.find_opt (fun form ->
           match head form with
           | Some "plan" | Some "plan-draft" -> true
           | _ -> false)
  in
  match root with
  | None ->
      json_assoc
        [
          ("file", json_string file);
          ("head", "null");
          ("hints", "{}");
          ("nodes", "[]");
          ("diagnostic_summary", json_string "missing plan/plan-draft root");
        ]
  | Some root ->
      let props = keyword_props ~start:1 root in
      let nodes = collect_forms "node" root |> List.map plan_node_contract_json in
      json_assoc
        [
          ("file", json_string file);
          ("head", json_opt_string (head root));
          ("hints", props_to_object_json ~keys:plan_hint_keys props);
          ("top_level", props_to_object_json props);
          ("nodes", json_array nodes);
        ]

let plan_contract_diagnostics file forms =
  let diagnostics = ref [] in
  let roots =
    forms
    |> List.filter (fun form ->
           match head form with
           | Some "plan" | Some "plan-draft" -> true
           | _ -> false)
  in
  (match roots with
  | [] ->
      diagnostics :=
        diag file (synthetic_loc file) "plan_contract.root_missing"
          "plan contract source must contain a (plan ...) or (plan-draft ...) root"
        :: !diagnostics
  | _ :: _ :: _ ->
      diagnostics :=
        diag file (loc_of (List.hd roots)) "plan_contract.multiple_roots"
          "plan contract source must contain exactly one plan root"
        :: !diagnostics
  | [ root ] ->
      let seen = Hashtbl.create 16 in
      collect_forms "node" root
      |> List.iter (fun node ->
             match prop_text ":id" (keyword_props ~start:1 node) with
             | None ->
                 diagnostics :=
                   diag file (loc_of node) "plan_contract.node_id_missing"
                     "plan node is missing :id"
                   :: !diagnostics
             | Some id ->
                 if Hashtbl.mem seen id then
                   diagnostics :=
                     diag file (loc_of node) "plan_contract.node_id_duplicate"
                       ("duplicate plan node :id " ^ id)
                     :: !diagnostics
                 else Hashtbl.add seen id ()));
  List.rev !diagnostics

let emit_contract_abi blueprint =
  try
    let resolved = Source_resolver.resolve_blueprint_file blueprint in
    let diagnostics = Schema_v3.validate blueprint [] in
    let payload =
      contract_abi_payload_json blueprint resolved.source_hash resolved.source_units
        resolved.root
    in
    print_endline
      (result_json
         ~extra:[
           Printf.sprintf {|"compiled":%s|}
             (compiled_envelope "missiond.contract-abi.v1"
                resolved.source_hash diagnostics payload);
         ]
         (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with
  | Reader_error (l, msg) ->
      let d = diag blueprint l "parse.error" msg in
      print_endline (result_json false [ d ]);
      1
  | Sys_error msg ->
      let d = diag blueprint (synthetic_loc blueprint) "io.error" msg in
      print_endline (result_json false [ d ]);
      1

let emit_plan_contract file =
  try
    let source = read_file file in
    let forms = Parser.parse_source file source in
    let diagnostics = plan_contract_diagnostics file forms in
    let payload = plan_contract_payload_json file forms in
    print_endline
      (result_json
         ~extra:[
           Printf.sprintf {|"compiled":%s|}
             (compiled_envelope "missiond.plan-contract.v1" (source_hash source)
                diagnostics payload);
         ]
         (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with
  | Reader_error (l, msg) ->
      let d = diag file l "parse.error" msg in
      print_endline (result_json false [ d ]);
      1
  | Sys_error msg ->
      let d = diag file (synthetic_loc file) "io.error" msg in
      print_endline (result_json false [ d ]);
      1

let check_plan_contract file =
  try
    let forms = Parser.parse_file file in
    let diagnostics = plan_contract_diagnostics file forms in
    print_endline (result_json (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with
  | Reader_error (l, msg) ->
      let d = diag file l "parse.error" msg in
      print_endline (result_json false [ d ]);
      1
  | Sys_error msg ->
      let d = diag file (synthetic_loc file) "io.error" msg in
      print_endline (result_json false [ d ]);
      1

let emit_ast file =
  try
    let forms = Parser.parse_file file in
    Printf.printf {|{"ok":true,"forms":[%s]}%s|}
      (forms |> List.map sexp_to_json |> String.concat ",")
      "\n";
    0
  with Reader_error (l, msg) ->
    let d = diag file l "parse.error" msg in
    print_endline (result_json false [ d ]);
    1

let emit_resolved_v3 blueprint =
  try
    let resolved = Source_resolver.resolve_blueprint_file blueprint in
    let resolved_source =
      resolved.forms |> List.map sexp_to_lisp |> String.concat "\n"
    in
    let payload =
      Printf.sprintf
        {|{"blueprint":%s,"source_units":%s,"resolved_source":%s,"forms":[%s]}|}
        (json_string blueprint)
        (Source_resolver.source_units_to_json resolved.source_units)
        (json_string resolved_source)
        (resolved.forms |> List.map sexp_to_json |> String.concat ",")
    in
    print_endline
      (result_json
         ~extra:[
           Printf.sprintf {|"compiled":%s|}
             (compiled_envelope "missiond.resolved-v3-blueprint.v1"
                resolved.source_hash [] payload);
         ]
         true []);
    0
  with
  | Reader_error (l, msg) ->
      let d = diag blueprint l "parse.error" msg in
      print_endline (result_json false [ d ]);
      1
  | Sys_error msg ->
      let d = diag blueprint (synthetic_loc blueprint) "io.error" msg in
      print_endline (result_json false [ d ]);
      1

let emit_v3 blueprint =
  try
    let resolved = Source_resolver.resolve_blueprint_file blueprint in
    let diagnostics = Schema_v3.validate blueprint [] in
    let forms = resolved.forms in
    let root = resolved.root in
    let surfaces =
      root |> Option.map v3_surfaces_to_json |> Option.value ~default:[]
    in
    let functions =
      root |> Option.map v3_functions_to_json |> Option.value ~default:[]
    in
    let payload =
      Printf.sprintf
        {|{"blueprint":%s,"source_units":%s,"surfaces":[%s],"functions":[%s],"forms":[%s]}|}
        (json_string blueprint)
        (Source_resolver.source_units_to_json resolved.source_units)
        (String.concat "," surfaces)
        (String.concat "," functions)
        (forms |> List.map sexp_to_json |> String.concat ",")
    in
    print_endline
      (result_json ~extra:[
        Printf.sprintf {|"compiled":%s|}
          (compiled_envelope "missiond.compiled-v3-blueprint.v1"
             resolved.source_hash diagnostics payload)
      ] (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with
  | Reader_error (l, msg) ->
      let d = diag blueprint l "parse.error" msg in
      print_endline (result_json false [ d ]);
      1
  | Sys_error msg ->
      let d = diag blueprint (synthetic_loc blueprint) "io.error" msg in
      print_endline (result_json false [ d ]);
      1

let emit_runtime_config blueprint =
  try
    let resolved = Source_resolver.resolve_blueprint_file blueprint in
    let root = resolved.root in
    let diagnostics =
      Schema_v3.validate blueprint []
      @ Workstation_schema.validate blueprint
      @
      match root with
      | Some root -> runtime_config_required_diagnostics blueprint root
      | None ->
          [
            diag blueprint (synthetic_loc blueprint) "root.missing"
              "missing missiond-blueprint root";
          ]
    in
    let payload =
      runtime_config_payload_json blueprint resolved.source_hash resolved.source_units
        root
    in
    print_endline
      (result_json
         ~extra:[
           Printf.sprintf {|"compiled":%s|}
             (compiled_envelope "missiond.compiled-runtime-config.v1"
                resolved.source_hash diagnostics payload);
         ]
         (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with
  | Reader_error (l, msg) ->
      let d = diag blueprint l "parse.error" msg in
      print_endline (result_json false [ d ]);
      1
  | Sys_error msg ->
      let d = diag blueprint (synthetic_loc blueprint) "io.error" msg in
      print_endline (result_json false [ d ]);
      1

let emit_semantic_ir blueprint =
  try
    let resolved = Source_resolver.resolve_blueprint_file blueprint in
    let hash = resolved.source_hash in
    let diagnostics = Schema_v3.validate blueprint [] in
    let root = resolved.root in
    let facts =
      root
      |> Option.map (semantic_facts hash blueprint resolved.source_units)
      |> Option.value ~default:[]
    in
    let payload =
      Printf.sprintf
        {|{"blueprint":%s,"source_units":%s,"facts":[%s],"fact_count":%d}|}
        (json_string blueprint)
        (Source_resolver.source_units_to_json resolved.source_units)
        (String.concat "," facts)
        (List.length facts)
    in
    print_endline
      (result_json ~extra:[
        Printf.sprintf {|"compiled":%s|}
          (compiled_envelope "missiond.semantic-ir.v1" hash diagnostics payload)
      ] (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with
  | Reader_error (l, msg) ->
      let d = diag blueprint l "parse.error" msg in
      print_endline (result_json false [ d ]);
      1
  | Sys_error msg ->
      let d = diag blueprint (synthetic_loc blueprint) "io.error" msg in
      print_endline (result_json false [ d ]);
      1

let emit_universe blueprint =
  try
    let resolved = Source_resolver.resolve_blueprint_file blueprint in
    let forms = resolved.forms in
    let diagnostics = Project_schema.validate blueprint in
    let root =
      match resolved.root with
      | Some root -> Some root
      | None -> find_root forms "missiond-blueprint"
    in
    let project_registry = Option.bind root (fun root -> find_child root "project-blueprint-registry") in
    let maturity_registry = Option.bind root (fun root -> find_child root "project-maturity-registry") in
    let projects =
      project_registry
      |> Option.map (fun node -> list_forms "project" node |> List.map project_entry_to_json)
      |> Option.value ~default:[]
    in
    let maturities =
      maturity_registry
      |> Option.map (fun node -> list_forms "maturity" node |> List.map maturity_entry_to_json)
      |> Option.value ~default:[]
    in
    let payload =
      Printf.sprintf {|{"blueprint":%s,"project_registry_present":%s,"maturity_registry_present":%s,"projects":[%s],"maturity":[%s]}|}
        (json_string blueprint)
        (if project_registry <> None then "true" else "false")
        (if maturity_registry <> None then "true" else "false")
        (String.concat "," projects)
        (String.concat "," maturities)
    in
    print_endline
      (result_json ~extra:[
        Printf.sprintf {|"compiled":%s|}
          (compiled_envelope "missiond.compiled-project-universe.v1"
             resolved.source_hash diagnostics payload)
      ] (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with Sys_error msg ->
    let d = diag blueprint (synthetic_loc blueprint) "io.error" msg in
    print_endline (result_json false [ d ]);
    1

let emit_workflows workflow_dir =
  try
    let files = read_sorted_files workflow_dir ".lisp" in
    let sources = files |> List.map read_file in
    let diagnostics = Workflow_schema.validate_dir workflow_dir in
    let payload =
      Printf.sprintf {|{"workflow_dir":%s,"files":[%s],"workflows":[%s]}|}
        (json_string workflow_dir)
        (files |> List.map json_string |> String.concat ",")
        (files |> List.map workflow_entry_to_json |> String.concat ",")
    in
    let combined_hash = source_hash (String.concat "\n" sources) in
    print_endline
      (result_json ~extra:[
        Printf.sprintf {|"compiled":%s|}
          (compiled_envelope "missiond.compiled-workflows.v1" combined_hash diagnostics payload)
      ] (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with Sys_error msg ->
    let d = diag workflow_dir (synthetic_loc workflow_dir) "io.error" msg in
    print_endline (result_json false [ d ]);
    1
