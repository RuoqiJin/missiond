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

let project_entry_to_json node =
  let props = keyword_props ~start:1 node in
  let checks =
    match prop ":checks" props with
    | Some value -> list_texts value
    | None -> []
  in
  Printf.sprintf
    {|{"id":%s,"kind":%s,"root":%s,"path":%s,"intent":%s,"backend":%s,"frontend":%s,"status":%s,"surface":%s,"checks":%s}|}
    (json_opt_string (prop_text ":id" props))
    (json_opt_string (prop_text ":kind" props))
    (json_opt_string (prop_text ":root" props))
    (json_opt_string (prop_text ":path" props))
    (json_opt_string (prop_text ":intent" props))
    (json_opt_string (prop_text ":backend" props))
    (json_opt_string (prop_text ":frontend" props))
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
        match prop ":steps" props with
        | Some value -> list_texts value
        | None -> []
      in
      let name =
        match children wf with
        | _ :: name_node :: _ -> atom_text name_node
        | _ -> None
      in
      let risk_gate_count =
        match prop ":risk-gates" props with
        | Some value -> count_forms "gate" value
        | None -> 0
      in
      let completion_criteria_count =
        match prop ":completion" props with
        | Some value -> count_forms "criterion" value
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

let emit_v3 blueprint =
  try
    let source = read_file blueprint in
    let diagnostics = Schema_v3.validate blueprint [] in
    let payload =
      Printf.sprintf {|{"blueprint":%s,"forms":[%s]}|}
        (json_string blueprint)
        (Parser.parse_source blueprint source |> List.map sexp_to_json |> String.concat ",")
    in
    print_endline
      (result_json ~extra:[
        Printf.sprintf {|"compiled":%s|}
          (compiled_envelope "missiond.compiled-v3-blueprint.v1" (source_hash source) diagnostics payload)
      ] (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with
  | Reader_error (l, msg) ->
      let d = diag blueprint l "parse.error" msg in
      print_endline (result_json false [ d ]);
      1
  | Sys_error msg ->
      let d = diag blueprint { line = 1; column = 1 } "io.error" msg in
      print_endline (result_json false [ d ]);
      1

let emit_universe blueprint =
  try
    let source = read_file blueprint in
    let forms = Parser.parse_source blueprint source in
    let diagnostics = Project_schema.validate blueprint in
    let root = find_root forms "missiond-blueprint" in
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
        (if contains_substring source "(project-blueprint-registry" then "true" else "false")
        (if contains_substring source "(project-maturity-registry" then "true" else "false")
        (String.concat "," projects)
        (String.concat "," maturities)
    in
    print_endline
      (result_json ~extra:[
        Printf.sprintf {|"compiled":%s|}
          (compiled_envelope "missiond.compiled-project-universe.v1" (source_hash source) diagnostics payload)
      ] (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with Sys_error msg ->
    let d = diag blueprint { line = 1; column = 1 } "io.error" msg in
    print_endline (result_json false [ d ]);
    1

let emit_workflows workflow_dir =
  try
    let files = read_sorted_files workflow_dir ".lisp" in
    let sources = files |> List.map read_file in
    let strict_workflows =
      [
        "project-ssot-convergence.lisp";
        "project-domain-hardening.lisp";
        "typed-lisp-compiler-convergence.lisp";
      ]
    in
    let diagnostics =
      files
      |> List.filter (fun file -> List.mem (Filename.basename file) strict_workflows)
      |> List.map Workflow_schema.validate
      |> List.flatten
    in
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
    let d = diag workflow_dir { line = 1; column = 1 } "io.error" msg in
    print_endline (result_json false [ d ]);
    1
