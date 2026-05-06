let assert_true label cond =
  if not cond then failwith ("assertion failed: " ^ label)

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
      assert_true "root line preserves comments" (loc.line = 3)
  | _ -> failwith "unexpected parser shape"

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

let () =
  test_parser_locations ();
  test_v3_missing_entry ();
  test_v3_step_order ();
  test_workflow_missing_risk_gate ();
  test_workflow_dir_validates_all_files ();
  test_auth_domain_requires_compatibility_ledger ();
  test_auth_structured_function_requires_runtime_projection ();
  test_project_function_requires_ordered_steps ();
  test_project_function_requires_surface ();
  test_project_dir_validates_active_blueprints ();
  test_project_dir_rejects_invalid_blueprint ();
  print_endline "missiond_lispc parser and validator golden tests passed"
