open Ast

let compiled_extra compiled = Printf.sprintf {|"compiled":%s|} compiled

let print_compiled_result diagnostics compiled =
  print_endline
    (result_json ~extra:[ compiled_extra compiled ] (diagnostics = []) diagnostics);
  if diagnostics = [] then 0 else 1

let parse_error_result file loc message =
  let d = diag file loc "parse.error" message in
  print_endline (result_json false [ d ]);
  1

let io_error_result file message =
  let d = diag file (synthetic_loc file) "io.error" message in
  print_endline (result_json false [ d ]);
  1

let emit_blueprint_projection blueprint compile =
  try
    let resolved = Source_resolver.resolve_blueprint_file blueprint in
    let compiled, diagnostics = compile blueprint resolved in
    print_compiled_result diagnostics compiled
  with
  | Reader_error (loc, message) -> parse_error_result blueprint loc message
  | Sys_error message -> io_error_result blueprint message
