open Ast

let emit_plan_contract file =
  try
    let source = read_file file in
    let forms = Parser.parse_source file source in
    let diagnostics = Emit_json.plan_contract_diagnostics file forms in
    let hash = source_hash source in
    let payload = Emit_json.plan_contract_payload_json file hash forms in
    let compiled =
      Emit_json.compiled_envelope Emit_json.plan_contract_schema_version hash
        diagnostics payload
    in
    Emit_projection_support.print_compiled_result diagnostics compiled
  with
  | Reader_error (loc, message) ->
      Emit_projection_support.parse_error_result file loc message
  | Sys_error message -> Emit_projection_support.io_error_result file message

let check_plan_contract file =
  try
    let forms = Parser.parse_file file in
    let diagnostics = Emit_json.plan_contract_diagnostics file forms in
    print_endline (result_json (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with
  | Reader_error (loc, message) ->
      Emit_projection_support.parse_error_result file loc message
  | Sys_error message -> Emit_projection_support.io_error_result file message
