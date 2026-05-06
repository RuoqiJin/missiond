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
    let diagnostics = Project_schema.validate blueprint in
    let payload =
      Printf.sprintf {|{"blueprint":%s,"project_registry_present":%s,"maturity_registry_present":%s}|}
        (json_string blueprint)
        (if contains_substring source "(project-blueprint-registry" then "true" else "false")
        (if contains_substring source "(project-maturity-registry" then "true" else "false")
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
      Printf.sprintf {|{"workflow_dir":%s,"files":[%s]}|}
        (json_string workflow_dir)
        (files |> List.map json_string |> String.concat ",")
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
