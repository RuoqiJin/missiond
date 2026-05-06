open Ast

let rec collect_forms named node =
  let here = if is_list node named then [ node ] else [] in
  match node with
  | List (_, _, xs) -> here @ (xs |> List.map (collect_forms named) |> List.flatten)
  | _ -> here

let form_id = function
  | List (_, _, _ :: id_node :: _) -> atom_text id_node
  | _ -> None

let collect_step_ids node = collect_forms "step" node |> List.filter_map form_id

let list_nonempty node = list_texts node <> [] || collect_step_ids node <> []

let count_named_or_list_entries named value =
  let named_count = collect_forms named value |> List.length in
  if named_count > 0 then named_count else List.length (list_texts value)

let validate_required_prop file workflow props key diagnostics =
  match prop key props with
  | Some value when list_nonempty value -> diagnostics
  | Some value ->
      diag file (loc_of value) ("workflow." ^ String.sub key 1 (String.length key - 1) ^ "_empty")
        (Printf.sprintf "%s must not be empty" key)
      :: diagnostics
  | None ->
      diag file (loc_of workflow) ("workflow." ^ String.sub key 1 (String.length key - 1) ^ "_missing")
        (Printf.sprintf "missing %s" key)
      :: diagnostics

let validate_steps file workflow props diagnostics =
  let declared =
    match prop ":steps" props with
    | Some value ->
        let direct = list_texts value in
        if direct <> [] then direct else collect_step_ids value
    | None -> []
  in
  let core_steps =
    match prop ":core" props with
    | Some core -> collect_step_ids core
    | None -> []
  in
  let embedded_steps =
    if declared <> [] then declared else collect_step_ids workflow
  in
  let diagnostics =
    if embedded_steps = [] then
      diag file (loc_of workflow) "workflow.steps_empty" "workflow has no steps"
      :: diagnostics
    else diagnostics
  in
  match (declared, core_steps) with
  | [], _ -> diagnostics
  | _, [] -> diagnostics
  | a, b when a = b -> diagnostics
  | a, b ->
      diag file (loc_of workflow) "workflow.steps_core_mismatch"
        (Printf.sprintf ":steps [%s] does not match :core step ids [%s]"
           (String.concat " " a) (String.concat " " b))
      :: diagnostics

let validate_counted_block file workflow props key form_name diagnostics =
  match prop key props with
  | Some value ->
      if count_named_or_list_entries form_name value > 0 then diagnostics
      else
        diag file (loc_of value) ("workflow." ^ String.sub key 1 (String.length key - 1) ^ "_empty")
          (Printf.sprintf "%s must contain at least one %s or vector entry" key form_name)
        :: diagnostics
  | None ->
      diag file (loc_of workflow) ("workflow." ^ String.sub key 1 (String.length key - 1) ^ "_missing")
        (Printf.sprintf "missing %s" key)
      :: diagnostics

let validate file =
  try
    let forms = Parser.parse_file file in
    let diagnostics = ref [] in
    let add d = diagnostics := d :: !diagnostics in
    let workflow = List.find_opt (fun n -> is_list n "workflow") forms in
    (match workflow with
    | None -> add (diag file { line = 1; column = 1 } "workflow.missing" "missing workflow root")
    | Some wf ->
        let props = keyword_props ~start:1 wf in
        if prop_text ":workflow_id" props = None then
          add (diag file (loc_of wf) "workflow.workflow_id_missing" "missing :workflow_id");
        if prop_text ":status" props = None then
          add (diag file (loc_of wf) "workflow.status_missing" "missing :status");
        let structural_diagnostics =
          []
          |> validate_required_prop file wf props ":source_plans"
          |> validate_required_prop file wf props ":steps"
          |> validate_steps file wf props
          |> validate_counted_block file wf props ":risk-gates" "gate"
          |> validate_counted_block file wf props ":completion" "criterion"
        in
        List.iter add structural_diagnostics);
    List.rev !diagnostics
  with
  | Reader_error (l, msg) -> [ diag file l "parse.error" msg ]
  | Sys_error msg -> [ diag file { line = 1; column = 1 } "io.error" msg ]

let workflow_files dir =
  Sys.readdir dir
  |> Array.to_list
  |> List.filter (fun name -> Filename.check_suffix name ".lisp")
  |> List.sort String.compare
  |> List.map (Filename.concat dir)

let validate_dir dir =
  try workflow_files dir |> List.map validate |> List.flatten
  with Sys_error msg -> [ diag dir { line = 1; column = 1 } "io.error" msg ]
