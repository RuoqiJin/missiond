let emit_contract_abi blueprint =
  Emit_projection_support.emit_blueprint_projection blueprint
    Emit_json.compiled_contract_abi_for_resolved
