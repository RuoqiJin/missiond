let assert_true label cond =
  if not cond then failwith ("assertion failed: " ^ label)

let contains_substring haystack needle =
  let haystack_len = String.length haystack in
  let needle_len = String.length needle in
  if needle_len = 0 then true
  else
    let rec loop index =
      if index + needle_len > haystack_len then false
      else if String.sub haystack index needle_len = needle then true
      else loop (index + 1)
    in
    loop 0

let write_temp name source =
  let file = Filename.temp_file name ".lisp" in
  let oc = open_out_bin file in
  output_string oc source;
  close_out oc;
  file

let with_temp name source f =
  let file = write_temp name source in
  Fun.protect ~finally:(fun () -> Sys.remove file) (fun () -> f file)

let with_temp_dir name f =
  let marker = Filename.temp_file name ".tmp" in
  Sys.remove marker;
  let dir = marker ^ ".d" in
  if Sys.command ("mkdir -p " ^ Filename.quote (Filename.concat dir "backend")) <> 0
  then failwith "failed to create temp dir";
  Fun.protect
    ~finally:(fun () -> ignore (Sys.command ("rm -rf " ^ Filename.quote dir)))
    (fun () -> f dir)

let has_code code diagnostics =
  List.exists (fun (d : Ast.diagnostic) -> d.code = code) diagnostics

let single_missiond_root source =
  match Parser.parse_source "runtime-config-fixture" source with
  | [ root ] when Ast.is_list root "missiond-blueprint" -> root
  | _ -> failwith "expected one missiond-blueprint root"

let test_parser_locations () =
  let forms =
    Parser.parse_source "golden"
      {|
; comment
(missiond-blueprint
  (pillar-flow-map
    (pillar workflow
      (function typed-lisp-compiler
        :surface typed-lisp-compiler
        :entry [check]
        :core ((step s1 :logic "parse"))
        :egress [diagnostics]))))
|}
  in
  assert_true "one top-level form" (List.length forms = 1);
  match forms with
  | Ast.List (loc, Ast.Paren, Ast.Atom (_, "missiond-blueprint") :: _) :: _ ->
      assert_true "root file is preserved" (loc.source_file = "golden");
      assert_true "root line preserves comments" (loc.line = 3)
  | _ -> failwith "unexpected parser shape"

let test_source_resolver_include () =
  with_temp_dir "v3-resolver" (fun dir ->
      let shards = Filename.concat dir "shards" in
      if Sys.command ("mkdir -p " ^ Filename.quote shards) <> 0 then
        failwith "failed to create shards dir";
      let blueprint = Filename.concat dir "missiond-blueprint.lisp" in
      let shard = Filename.concat shards "pillar-flow-map.lisp" in
      let oc = open_out_bin blueprint in
      output_string oc
        {|
(missiond-blueprint
  (implementation-map
    (surface typed-lisp-compiler :status code-aligned :code ["compiler.ml"]))
  (include "shards/pillar-flow-map.lisp"))
|};
      close_out oc;
      let oc = open_out_bin shard in
      output_string oc
        {|
(pillar-flow-map
  (pillar workflow
    (function typed-lisp-compiler
      :surface typed-lisp-compiler
      :entry [check]
      :core ((step s1 :logic "parse"))
      :egress [diagnostics])))

(state-machines
  (state-machine fixture
    :states [ready done]
    :anchor secret-store://fixture/shard-only-anchor))
|};
      close_out oc;
      let resolved = Source_resolver.resolve_blueprint_file blueprint in
      assert_true "root and shard source units are present"
        (List.length resolved.source_units = 2);
      let resolved_source =
        resolved.forms |> List.map Ast.sexp_to_lisp |> String.concat "\n"
      in
      assert_true "resolved source contains shard-only anchor"
        (contains_substring resolved_source
           "secret-store://fixture/shard-only-anchor");
      match resolved.root with
      | Some root ->
          begin
          (match Ast.find_child root "pillar-flow-map" with
          | Some flow ->
              assert_true "shard source file is preserved"
                ((Ast.loc_of flow).source_file = shard)
          | None -> failwith "included pillar-flow-map missing");
          match Ast.find_child root "state-machines" with
          | Some states ->
              assert_true "second shard form source file is preserved"
                ((Ast.loc_of states).source_file = shard)
          | None -> failwith "second included shard form missing"
          end
      | None -> failwith "resolved root missing")

let test_source_resolver_include_shard_index () =
  with_temp_dir "v3-resolver-index" (fun dir ->
      let shards = Filename.concat dir "shards" in
      if Sys.command ("mkdir -p " ^ Filename.quote shards) <> 0 then
        failwith "failed to create shards dir";
      let blueprint = Filename.concat dir "missiond-blueprint.lisp" in
      let index = Filename.concat shards "index.lisp" in
      let shard = Filename.concat shards "pillar-flow-map.lisp" in
      let oc = open_out_bin blueprint in
      output_string oc
        {|
(missiond-blueprint
  (implementation-map
    (surface typed-lisp-compiler :status code-aligned :code ["compiler.ml"]))
  (include-shard-index "shards/index.lisp"))
|};
      close_out oc;
      let oc = open_out_bin index in
      output_string oc
        {|
(missiond-blueprint-shards
  (shard ignored
    :status review-only
    :path "shards/ignored.lisp")
  (shard pillar-flow-map
    :status compiler-active
    :path "shards/pillar-flow-map.lisp"))
|};
      close_out oc;
      let oc = open_out_bin shard in
      output_string oc
        {|
(pillar-flow-map
  (pillar workflow
    (function typed-lisp-compiler
      :surface typed-lisp-compiler
      :entry [check]
      :core ((step s1 :logic "parse"))
      :egress [diagnostics])))
|};
      close_out oc;
      let resolved = Source_resolver.resolve_blueprint_file blueprint in
      assert_true "root, index, and compiler-active shard source units are present"
        (List.length resolved.source_units = 3);
      let resolved_source =
        resolved.forms |> List.map Ast.sexp_to_lisp |> String.concat "\n"
      in
      assert_true "resolved source contains indexed shard"
        (contains_substring resolved_source "(pillar-flow-map");
      match resolved.root with
      | Some root -> (
          match Ast.find_child root "pillar-flow-map" with
          | Some flow ->
              assert_true "indexed shard source file is preserved"
                ((Ast.loc_of flow).source_file = shard)
          | None -> failwith "indexed pillar-flow-map missing")
      | None -> failwith "resolved root missing")

let test_source_resolver_rejects_nested_include () =
  with_temp_dir "v3-resolver-nested" (fun dir ->
      let shards = Filename.concat dir "shards" in
      if Sys.command ("mkdir -p " ^ Filename.quote shards) <> 0 then
        failwith "failed to create shards dir";
      let blueprint = Filename.concat dir "missiond-blueprint.lisp" in
      let shard = Filename.concat shards "nested.lisp" in
      let oc = open_out_bin blueprint in
      output_string oc
        {|
(missiond-blueprint
  (include "shards/nested.lisp"))
|};
      close_out oc;
      let oc = open_out_bin shard in
      output_string oc
        {|
(include "shards/other.lisp")
|};
      close_out oc;
      let rejected =
        try
          ignore (Source_resolver.resolve_blueprint_file blueprint);
          false
        with Ast.Reader_error _ -> true
      in
      assert_true "nested shard includes are rejected" rejected)

let test_v3_missing_entry () =
  with_temp "missiond-v3-invalid"
    {|
(missiond-blueprint
  (implementation-map
    (surface typed-lisp-compiler))
  (pillar-flow-map
    (pillar workflow
      (function typed-lisp-compiler
        :surface typed-lisp-compiler
        :core ((step s1 :logic "parse"))
        :egress [diagnostics]))))
|}
    (fun file ->
      let diagnostics = Schema_v3.validate file [ "typed-lisp-compiler" ] in
      assert_true "missing entry is diagnosed"
        (has_code "function.entry_missing" diagnostics))

let test_v3_step_order () =
  with_temp "missiond-v3-invalid"
    {|
(missiond-blueprint
  (implementation-map
    (surface typed-lisp-compiler))
  (pillar-flow-map
    (pillar workflow
      (function typed-lisp-compiler
        :surface typed-lisp-compiler
        :entry [check]
        :core ((step s2 :logic "parse"))
        :egress [diagnostics]))))
|}
    (fun file ->
      let diagnostics = Schema_v3.validate file [ "typed-lisp-compiler" ] in
      assert_true "unordered step is diagnosed"
        (has_code "core.step_order" diagnostics))

let test_policy_clause_requires_structured_fields () =
  with_temp "missiond-v3-policy-invalid"
    {|
(missiond-blueprint
  (implementation-map
    (surface mission_request))
  (pillar-flow-map
    (pillar request
      (function mission-request
        :surface mission_request
        :entry [request]
        :core ((step s1 :logic "route"))
        :egress [review-packet])))
  (policy-clause missing-fields
    :applies-to [mission_request])
  (policy-clause duplicate-policy
    :owner mission_request
    :applies-to [mission_request]
    :must [route-through-review-gate])
  (policy-clause duplicate-policy
    :owner mission_request
    :applies-to [mission_request]
    :must [route-through-review-gate]))
|}
    (fun file ->
      let diagnostics = Schema_v3.validate file [ "mission_request" ] in
      assert_true "missing policy owner is diagnosed"
        (has_code "policy_clause.owner_missing" diagnostics);
      assert_true "missing policy must is diagnosed"
        (has_code "policy_clause.must_missing" diagnostics);
      assert_true "duplicate policy id is diagnosed"
        (has_code "policy_clause.duplicate_id" diagnostics))

let test_workflow_missing_risk_gate () =
  with_temp "missiond-workflow-invalid"
    {|
(workflow typed-lisp-compiler-convergence
  :workflow_id typed-lisp-compiler-convergence
  :status active
  :source_plans [lisp-ssot-v3]
  :steps [s1]
  :completion (:checks ["node scripts/check-typed-lisp-compiler.mjs"])
  :core ((step s1 :logic "compile")))
|}
    (fun file ->
      let diagnostics = Workflow_schema.validate file in
      assert_true "missing risk gate is diagnosed"
        (has_code "workflow.risk-gates_missing" diagnostics))

let test_workflow_dir_validates_all_files () =
  with_temp_dir "workflow-dir" (fun dir ->
      let valid = Filename.concat dir "valid.lisp" in
      let invalid = Filename.concat dir "invalid.lisp" in
      let oc = open_out_bin valid in
      output_string oc
        {|
(workflow valid-workflow
  :workflow_id valid-workflow
  :status active
  :source_plans [fixture]
  :steps [s1]
  :risk-gates [manual]
  :completion (:checks ["ok"])
  :core ((step s1 :logic "run")))
|};
      close_out oc;
      let oc = open_out_bin invalid in
      output_string oc
        {|
(workflow invalid-workflow
  :workflow_id invalid-workflow
  :status active
  :source_plans [fixture]
  :steps [s1]
  :core ((step s1 :logic "run")))
|};
      close_out oc;
      let diagnostics = Workflow_schema.validate_dir dir in
      assert_true "workflow dir catches invalid file"
        (has_code "workflow.risk-gates_missing" diagnostics))

let test_auth_domain_requires_compatibility_ledger () =
  let source =
    String.concat "\n"
      [
        "tenant application product product_user product_user_group";
        "runtime product-access-policy identity-binding token-claim-contract";
        "outbox adapter";
      ]
  in
  let diagnostics = Project_schema.validate_auth_domain_source "auth" source in
  assert_true "missing compatibility ledger is diagnosed"
    (has_code "auth.compatibility_ledger" diagnostics)

let test_auth_structured_function_requires_runtime_projection () =
  let forms =
    Parser.parse_source "auth-struct"
      {|
(function product-access-policy
  :entry "context"
  :core ((step s1 :logic "load policy"))
  :egress "decision"
  :surfaces ["src/services/product_policy.rs"])
|}
  in
  match forms with
  | form :: _ ->
      let diagnostics =
        Project_schema.validate_auth_structured_form "auth" "function" form
      in
      assert_true "missing runtime projection is diagnosed"
        (has_code "auth.runtime_projection_missing" diagnostics)
  | [] -> failwith "missing parsed auth function"

let test_project_function_requires_ordered_steps () =
  with_temp "project-blueprint-invalid"
    {|
(jarvis-blueprint
  (function runtime-config
    :surface runtime-config
    :entry [config]
    :core ((step s2 :logic "load config"))
    :egress [runtime]))
|}
    (fun file ->
      let diagnostics = Project_schema.validate file in
      assert_true "unordered project step is diagnosed"
        (has_code "project.core_step_order" diagnostics))

let test_project_function_requires_surface () =
  with_temp "project-blueprint-invalid"
    {|
(jarvis-blueprint
  (function runtime-config
    :entry [config]
    :core ((step s1 :logic "load config"))
    :egress [runtime]))
|}
    (fun file ->
      let diagnostics = Project_schema.validate file in
      assert_true "missing project surface is diagnosed"
        (has_code "project.surface_missing" diagnostics))

let test_project_dir_validates_active_blueprints () =
  with_temp_dir "project-dir" (fun dir ->
      let blueprint = Filename.concat dir "backend/demo-blueprint.lisp" in
      let oc = open_out_bin blueprint in
      output_string oc
        {|
(demo-blueprint
  (function runtime-config
    :entry [config]
    :core ((step s1 :logic "load config"))
    :egress [runtime]
    :surfaces ["src/main.rs"]))
|};
      close_out oc;
      let diagnostics = Project_schema.validate_project_dir dir in
      assert_true "valid project dir has no diagnostics" (diagnostics = []))

let test_project_dir_rejects_invalid_blueprint () =
  with_temp_dir "project-dir-invalid" (fun dir ->
      let blueprint = Filename.concat dir "backend/demo-blueprint.lisp" in
      let oc = open_out_bin blueprint in
      output_string oc
        {|
(demo-blueprint
  (function runtime-config
    :entry [config]
    :core ((step s1 :logic "load config"))
    :surfaces ["src/main.rs"]))
|};
      close_out oc;
      let diagnostics = Project_schema.validate_project_dir dir in
      assert_true "missing egress in project dir is diagnosed"
        (has_code "project.egress_missing" diagnostics))

let test_runtime_config_payload_shape () =
  let root =
    single_missiond_root
      {|
(missiond-blueprint
  (workstation-config)
  (flow-runtime-policy :slot-task-default-model "sonnet")
  (router-runtime-policy :default-chat-model "router"))
|}
  in
  let payload =
    Emit_json.runtime_config_payload_json "runtime-config-fixture.lisp"
      "fixture-hash" [] (Some root)
  in
  let envelope =
    Emit_json.compiled_envelope "missiond.compiled-runtime-config.v1"
      "fixture-hash" [] payload
  in
  List.iter
    (fun needle ->
      assert_true ("runtime config payload contains " ^ needle)
        (contains_substring envelope needle))
    [
      "\"schema_version\":\"missiond.compiled-runtime-config.v1\"";
      "\"workstation\"";
      "\"flow\"";
      "\"compute\"";
      "\"minimax\"";
      "\"router\"";
      "\"cascade\"";
      "\"projectRegistry\"";
      "\"capabilityGovernance\"";
      "\"memoryKb\"";
      "\"conversationIngestion\"";
      "\"autopilot\"";
      "\"learningEngine\"";
    ]

let test_runtime_config_missing_policy_diagnostics () =
  let root =
    single_missiond_root
      {|
(missiond-blueprint
  (autopilot-policy :stale-conversation-minutes 15))
|}
  in
  let diagnostics =
    Emit_json.runtime_config_required_diagnostics "runtime-config-invalid.lisp"
      root
  in
  assert_true "missing runtime policy is diagnosed"
    (has_code "runtime_config.policy_missing" diagnostics);
  assert_true "missing runtime policy field is diagnosed"
    (has_code "runtime_config.prop_missing" diagnostics)

let () =
  test_parser_locations ();
  test_source_resolver_include ();
  test_source_resolver_include_shard_index ();
  test_source_resolver_rejects_nested_include ();
  test_v3_missing_entry ();
  test_v3_step_order ();
  test_policy_clause_requires_structured_fields ();
  test_workflow_missing_risk_gate ();
  test_workflow_dir_validates_all_files ();
  test_auth_domain_requires_compatibility_ledger ();
  test_auth_structured_function_requires_runtime_projection ();
  test_project_function_requires_ordered_steps ();
  test_project_function_requires_surface ();
  test_project_dir_validates_active_blueprints ();
  test_project_dir_rejects_invalid_blueprint ();
  test_runtime_config_payload_shape ();
  test_runtime_config_missing_policy_diagnostics ();
  print_endline "missiond_lispc parser and validator golden tests passed"
