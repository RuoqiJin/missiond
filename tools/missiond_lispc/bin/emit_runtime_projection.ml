open Ast

let emit_runtime_config blueprint =
  Emit_projection_support.emit_blueprint_projection blueprint
    Emit_json.compiled_runtime_config_for_resolved

let compile_v3_runtime blueprint workflow_dir genome_dir =
  try
    let resolved = Source_resolver.resolve_blueprint_file blueprint in
    let v3, v3_diagnostics =
      Emit_json.compiled_v3_for_resolved blueprint resolved
    in
    let runtime_config, runtime_diagnostics =
      Emit_json.compiled_runtime_config_for_resolved blueprint resolved
    in
    let semantic_ir, semantic_diagnostics =
      Emit_json.compiled_semantic_ir_for_resolved blueprint resolved
    in
    let contract_abi, contract_diagnostics =
      Emit_json.compiled_contract_abi_for_resolved blueprint resolved
    in
    let universe, universe_diagnostics =
      Emit_json.compiled_universe_for_resolved blueprint resolved
    in
    let workflows, workflow_diagnostics = Emit_json.compiled_workflows workflow_dir in
    let genomes, genome_diagnostics = Genome_schema.compiled_genomes genome_dir in
    let diagnostics =
      v3_diagnostics @ runtime_diagnostics @ semantic_diagnostics
      @ contract_diagnostics @ universe_diagnostics @ workflow_diagnostics
      @ genome_diagnostics
    in
    let payload =
      Printf.sprintf
        {|{"blueprint":%s,"workflow_dir":%s,"genome_dir":%s,"targets":{"v3":%s,"runtime-config":%s,"semantic-ir":%s,"contract-abi":%s,"universe":%s,"workflows":%s,"genomes":%s}}|}
        (json_string blueprint) (json_string workflow_dir) (json_string genome_dir)
        v3 runtime_config semantic_ir contract_abi universe workflows genomes
    in
    let compiled =
      Emit_json.compiled_envelope "missiond.compile-v3-runtime-bundle.v1"
        resolved.source_hash diagnostics payload
    in
    Emit_projection_support.print_compiled_result diagnostics compiled
  with
  | Reader_error (loc, message) ->
      Emit_projection_support.parse_error_result blueprint loc message
  | Sys_error message -> Emit_projection_support.io_error_result blueprint message
