let emit_universe blueprint =
  Emit_projection_support.emit_blueprint_projection blueprint
    Emit_json.compiled_universe_for_resolved
