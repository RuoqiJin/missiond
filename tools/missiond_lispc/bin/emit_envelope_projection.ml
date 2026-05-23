open Ast

let emit_ast file =
  try
    let forms = Parser.parse_file file in
    Printf.printf {|{"ok":true,"forms":[%s]}%s|}
      (forms |> List.map sexp_to_json |> String.concat ",")
      "\n";
    0
  with Reader_error (loc, message) ->
    Emit_projection_support.parse_error_result file loc message

let emit_resolved_v3 blueprint =
  try
    let resolved = Source_resolver.resolve_blueprint_file blueprint in
    let resolved_source =
      resolved.forms |> List.map sexp_to_lisp |> String.concat "\n"
    in
    let payload =
      Printf.sprintf
        {|{"blueprint":%s,"source_units":%s,"source_domains":%s,"resolved_source":%s,"forms":[%s]}|}
        (json_string blueprint)
        (Source_resolver.source_units_to_json resolved.source_units)
        (Source_resolver.source_domains_to_json resolved.source_domains)
        (json_string resolved_source)
        (resolved.forms |> List.map sexp_to_json |> String.concat ",")
    in
    print_endline
      (result_json
         ~extra:[
           Emit_projection_support.compiled_extra
             (Emit_json.compiled_envelope "missiond.resolved-v3-blueprint.v1"
                resolved.source_hash [] payload);
         ]
         true []);
    0
  with
  | Reader_error (loc, message) ->
      Emit_projection_support.parse_error_result blueprint loc message
  | Sys_error message -> Emit_projection_support.io_error_result blueprint message

let emit_v3 blueprint =
  Emit_projection_support.emit_blueprint_projection blueprint
    Emit_json.compiled_v3_for_resolved
