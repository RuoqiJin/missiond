let emit_semantic_ir blueprint =
  Emit_projection_support.emit_blueprint_projection blueprint
    Emit_json.compiled_semantic_ir_for_resolved
