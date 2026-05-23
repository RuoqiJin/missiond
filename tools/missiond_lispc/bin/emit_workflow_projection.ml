let emit_workflows workflow_dir =
  try
    let compiled, diagnostics = Emit_json.compiled_workflows workflow_dir in
    Emit_projection_support.print_compiled_result diagnostics compiled
  with Sys_error message ->
    Emit_projection_support.io_error_result workflow_dir message
