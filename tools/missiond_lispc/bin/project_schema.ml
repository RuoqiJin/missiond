open Ast

let rec collect_forms named node =
  let here = if is_list node named then [ node ] else [] in
  match node with
  | List (_, _, xs) -> here @ (xs |> List.map (collect_forms named) |> List.flatten)
  | _ -> here

let form_id = function
  | List (_, _, _ :: id_node :: _) -> atom_text id_node
  | _ -> None

let nonempty_list_prop key props =
  match prop key props with
  | Some value -> list_texts value <> []
  | None -> false

let has_suffix s suffix =
  let ls = String.length s in
  let lx = String.length suffix in
  ls >= lx && String.sub s (ls - lx) lx = suffix

let prop_or_child key child props form =
  match prop key props with Some node -> Some node | None -> find_child form child

let has_prop_or_child key child props form = prop_or_child key child props form <> None

let parse_step_id id =
  let len = String.length id in
  let rec split i =
    if i >= len then None
    else
      let c = id.[i] in
      if c >= '0' && c <= '9' then
        let prefix = String.sub id 0 i in
        let digits = String.sub id i (len - i) in
        try Some (prefix, int_of_string digits) with Failure _ -> None
      else split (i + 1)
  in
  split 0

let expected_step_id first_prefix first_number index =
  first_prefix ^ string_of_int (first_number + index)

let validate_project_core_steps file form_id_label form =
  let props = keyword_props ~start:2 form in
  match prop_or_child ":core" "core" props form with
  | Some (List (_, _, _) as core) ->
      let steps = collect_forms "step" core in
      if steps = [] then
        [ diag file (loc_of core) "project.core_empty"
            (Printf.sprintf "%s :core must contain at least one ordered step" form_id_label)
        ]
      else (
        let first =
          match steps with
          | first_step :: _ -> (
              match children first_step with
              | _ :: id_node :: _ -> Option.bind (atom_text id_node) parse_step_id
              | _ -> None)
          | [] -> None
        in
        steps
        |> List.mapi (fun i step ->
               let expected =
                 match first with
                 | Some (prefix, n) when n = 0 || n = 1 -> expected_step_id prefix n i
                 | _ -> "s" ^ string_of_int (i + 1)
               in
               let got =
                 match children step with _ :: id_node :: _ -> atom_text id_node | _ -> None
               in
               let step_props = keyword_props ~start:2 step in
               let logic =
                 match prop_text ":logic" step_props with
                 | Some value -> Some value
                 | None -> (
                     match children step with
                     | _ :: _ :: logic_node :: _ -> atom_text logic_node
                     | _ -> None)
               in
               let diagnostics =
                 if got = Some expected then []
                 else
                   [ diag file (loc_of step) "project.core_step_order"
                       (Printf.sprintf "%s step %d must be %s" form_id_label
                          (i + 1) expected)
                   ]
               in
               match logic with
               | Some value when String.trim value <> "" -> diagnostics
               | _ ->
                   diag file (loc_of step) "project.core_step_logic"
                     (Printf.sprintf "%s step %s must declare :logic" form_id_label expected)
                   :: diagnostics)
        |> List.flatten)
  | Some node ->
      [ diag file (loc_of node) "project.core_invalid"
          (Printf.sprintf "%s must declare list :core" form_id_label)
      ]
  | None ->
      [ diag file (loc_of form) "project.core_missing"
          (Printf.sprintf "%s missing :core" form_id_label)
      ]

let validate_project_function_form file form =
  let props = keyword_props ~start:2 form in
  let id = form_id form in
  let label =
    match id with Some id -> "function " ^ id | None -> "function <missing-id>"
  in
  let diagnostics = ref [] in
  let add d = diagnostics := d :: !diagnostics in
  if id = None then
    add (diag file (loc_of form) "project.function_id_missing" (label ^ " missing id"));
  if not (has_prop_or_child ":entry" "entry" props form) then
    add (diag file (loc_of form) "project.entry_missing" (label ^ " missing :entry"));
  validate_project_core_steps file label form |> List.iter add;
  if not (has_prop_or_child ":egress" "egress" props form) then
    add (diag file (loc_of form) "project.egress_missing" (label ^ " missing :egress"));
  if
    prop ":surface" props = None
    && not (nonempty_list_prop ":surfaces" props)
    && find_child form "surface" = None
    && find_child form "surfaces" = None
  then
    add (diag file (loc_of form) "project.surface_missing" (label ^ " missing :surface or non-empty :surfaces"));
  List.rev !diagnostics

let validate_project_root_form file form =
  let props = keyword_props ~start:2 form in
  let root_label = match head form with Some value -> value | None -> "project-root" in
  let diagnostics = ref [] in
  let add d = diagnostics := d :: !diagnostics in
  if not (has_prop_or_child ":entry" "entry" props form) then
    add (diag file (loc_of form) "project.entry_missing" (root_label ^ " missing :entry"));
  validate_project_core_steps file root_label form |> List.iter add;
  if not (has_prop_or_child ":egress" "egress" props form) then
    add (diag file (loc_of form) "project.egress_missing" (root_label ^ " missing :egress"));
  List.rev !diagnostics

let validate file =
  try
    let resolved = Source_resolver.resolve_blueprint_file file in
    let forms = resolved.forms in
    let diagnostics = ref [] in
    let add d = diagnostics := d :: !diagnostics in
    (match find_root forms "missiond-blueprint" with
    | None ->
        if forms = [] then
          add
            (diag file (synthetic_loc file) "project.root_missing"
               "missing Lisp root");
        let functions =
          forms |> List.map (collect_forms "function") |> List.flatten
        in
        functions
        |> List.iter (fun form -> validate_project_function_form file form |> List.iter add);
        if functions = [] then
          let project_like_root =
            forms
            |> List.filter_map head
            |> List.exists (fun h -> has_suffix h "blueprint" || has_suffix h "intent")
          in
          if project_like_root then
            add (diag file (synthetic_loc file) "project.function_missing"
                   "project blueprint/intent must declare at least one function")
    | Some root ->
        if find_child root "project-maturity-registry" = None then
          add (diag file (loc_of root) "project.maturity_missing" "missing project-maturity-registry");
        if find_child root "project-blueprint-registry" = None then
          add (diag file (loc_of root) "project.registry_missing" "missing project-blueprint-registry"));
    List.rev !diagnostics
  with
  | Reader_error (l, msg) -> [ diag file l "parse.error" msg ]
  | Sys_error msg -> [ diag file (synthetic_loc file) "io.error" msg ]

let active_project_lisp_file file =
  let base = Filename.basename file in
  has_suffix base "blueprint.lisp"
  || has_suffix base "runtime-projection.lisp"
  || has_suffix base "implementation-map.lisp"
  || has_suffix base "compatibility-ledger.lisp"

let validate_project_forms_in_file file forms =
  let diagnostics = ref [] in
  let add d = diagnostics := d :: !diagnostics in
  let functions = forms |> List.map (collect_forms "function") |> List.flatten in
  functions
  |> List.iter (fun form -> validate_project_function_form file form |> List.iter add);
  List.rev !diagnostics

let validate_project_file file =
  try Parser.parse_file file |> validate_project_forms_in_file file
  with
  | Reader_error (l, msg) -> [ diag file l "parse.error" msg ]
  | Sys_error msg -> [ diag file (synthetic_loc file) "io.error" msg ]

let rec lisp_files_under dir =
  Sys.readdir dir
  |> Array.to_list
  |> List.map (Filename.concat dir)
  |> List.map (fun path ->
         if Sys.is_directory path then lisp_files_under path
         else if Filename.check_suffix path ".lisp" then [ path ]
         else [])
  |> List.flatten

let validate_project_dir dir =
  try
    let files =
      lisp_files_under dir
      |> List.filter active_project_lisp_file
      |> List.sort String.compare
    in
    let diagnostics = ref [] in
    let add d = diagnostics := d :: !diagnostics in
    if files = [] then
      add
        (diag dir (synthetic_loc dir) "project.lisp_missing"
           "project .missiond directory has no active Lisp SSOT files");
    let structured_count = ref 0 in
    files
    |> List.iter (fun file ->
           match Parser.parse_file file with
           | forms ->
               let functions =
                 forms |> List.map (collect_forms "function") |> List.flatten
               in
               structured_count := !structured_count + List.length functions;
               validate_project_forms_in_file file forms |> List.iter add;
               if functions = [] then
                 forms
                 |> List.iter (fun root ->
                        match head root with
                        | Some h when has_suffix h "blueprint" -> (
                            let props = keyword_props ~start:2 root in
                            if
                              has_prop_or_child ":entry" "entry" props root
                              && has_prop_or_child ":core" "core" props root
                              && has_prop_or_child ":egress" "egress" props root
                            then (
                              structured_count := !structured_count + 1;
                              validate_project_root_form file root |> List.iter add))
                        | _ -> ())
           | exception Reader_error (loc, msg) -> add (diag file loc "parse.error" msg)
           | exception Sys_error msg ->
               add (diag file (synthetic_loc file) "io.error" msg));
    if !structured_count = 0 then
      add
        (diag dir (synthetic_loc dir) "project.function_missing"
           "project .missiond active SSOT files must declare at least one function or structured blueprint root");
    List.rev !diagnostics
  with Sys_error msg -> [ diag dir (synthetic_loc dir) "io.error" msg ]

let validate_auth_domain_source file source =
  let required =
    [
      ("auth.domain.tenant", "tenant");
      ("auth.domain.application", "application");
      ("auth.domain.product", "product");
      ("auth.domain.product_user", "product_user");
      ("auth.domain.product_user_group", "product_user_group");
      ("auth.runtime_registration", "runtime");
      ("auth.product_policy", "product-access-policy");
      ("auth.identity_binding", "identity-binding");
      ("auth.token_claim", "token-claim-contract");
      ("auth.event_outbox", "outbox");
      ("auth.adapter_contract", "adapter");
      ("auth.compatibility_ledger", "compatibility");
    ]
  in
  required
  |> List.filter_map (fun (code, needle) ->
         if contains_substring source needle then None
         else
           Some
             (diag file (synthetic_loc file) code
                (Printf.sprintf "auth domain SSOT missing required concept: %s" needle)))

let validate_core_steps file form_id_label form =
  let props =
    keyword_props ~start:(if is_list form "mapping" then 1 else 2) form
  in
  match prop ":core" props with
  | Some (List (_, _, xs) as core) ->
      let steps = xs |> List.filter (fun node -> is_list node "step") in
      if steps = [] then
        [ diag file (loc_of core) "auth.core_empty"
            (Printf.sprintf "%s :core must contain at least one step" form_id_label)
        ]
      else
        steps
        |> List.mapi (fun i step ->
               let expected = "s" ^ string_of_int (i + 1) in
               let got =
                 match children step with _ :: id_node :: _ -> atom_text id_node | _ -> None
               in
               let step_props = keyword_props ~start:2 step in
               let logic = prop_text ":logic" step_props in
               let diagnostics =
                 if got = Some expected then []
                 else
                   [ diag file (loc_of step) "auth.core_step_order"
                       (Printf.sprintf "%s step %d must be %s" form_id_label
                          (i + 1) expected)
                   ]
               in
               match logic with
               | Some value when String.trim value <> "" -> diagnostics
               | _ ->
                   diag file (loc_of step) "auth.core_step_logic"
                     (Printf.sprintf "%s step %s must declare :logic" form_id_label expected)
                   :: diagnostics)
        |> List.flatten
  | Some node ->
      [ diag file (loc_of node) "auth.core_invalid"
          (Printf.sprintf "%s must declare list :core" form_id_label)
      ]
  | None ->
      [ diag file (loc_of form) "auth.core_missing"
          (Printf.sprintf "%s missing :core" form_id_label)
      ]

let validate_auth_structured_form file kind form =
  let props = keyword_props ~start:(if kind = "mapping" then 1 else 2) form in
  let id =
    if kind = "mapping" then prop_text ":surface" props else form_id form
  in
  let label =
    match id with
    | Some id -> kind ^ " " ^ id
    | None -> kind ^ " <missing-id>"
  in
  let diagnostics = ref [] in
  let add d = diagnostics := d :: !diagnostics in
  if id = None then add (diag file (loc_of form) "auth.form_id_missing" (label ^ " missing id"));
  if prop ":entry" props = None then add (diag file (loc_of form) "auth.entry_missing" (label ^ " missing :entry"));
  validate_core_steps file label form |> List.iter add;
  if prop ":egress" props = None then add (diag file (loc_of form) "auth.egress_missing" (label ^ " missing :egress"));
  if not (nonempty_list_prop ":surfaces" props) then
    add (diag file (loc_of form) "auth.surfaces_missing" (label ^ " missing non-empty :surfaces"));
  if prop ":runtime-projection" props = None then
    add (diag file (loc_of form) "auth.runtime_projection_missing" (label ^ " missing :runtime-projection"));
  List.rev !diagnostics

let form_ids named forms =
  forms
  |> List.map (collect_forms named)
  |> List.flatten
  |> List.filter_map (fun form ->
         if named = "mapping" then keyword_props ~start:1 form |> prop_text ":surface"
         else form_id form)

let require_ids file forms named required =
  let ids = form_ids named forms in
  required
  |> List.filter_map (fun id ->
         if List.mem id ids then None
         else
           Some
             (diag file (synthetic_loc file)
                ("auth." ^ named ^ "_missing")
                (Printf.sprintf "missing auth %s %s" named id)))

let validate_auth_domain_structured_dir dir =
  let file rel = Filename.concat dir rel in
  let required_files =
    [
      "backend/auth-domain-blueprint.lisp";
      "backend/auth-policy-blueprint.lisp";
      "backend/auth-flow-blueprint.lisp";
      "backend/auth-token-blueprint.lisp";
      "backend/auth-event-blueprint.lisp";
      "backend/auth-runtime-projection.lisp";
      "backend/auth-implementation-map.lisp";
      "backend/auth-compatibility-ledger.lisp";
    ]
  in
  let diagnostics = ref [] in
  let add d = diagnostics := d :: !diagnostics in
  let parsed_files =
    required_files
    |> List.filter_map (fun rel ->
           let path = file rel in
           if not (Sys.file_exists path) then (
             add
               (diag path (synthetic_loc path) "auth.shard_missing"
                  "missing required auth shard");
             None)
           else
             try Some (path, Parser.parse_file path)
             with
             | Reader_error (loc, msg) ->
                 add (diag path loc "parse.error" msg);
                 None
             | Sys_error msg ->
                 add (diag path (synthetic_loc path) "io.error" msg);
                 None)
  in
  let all_forms = parsed_files |> List.map snd |> List.flatten in
  [
    ("function", [
      "runtime-registration-domain";
      "product-access-policy";
      "admin-capability-matrix";
      "compatibility-policy";
      "identity-binding-state-machine";
      "dcr-runtime-registration";
      "login-provider-flows";
      "token-claim-contract";
      "grant-state-machine";
      "auth-event-schema";
      "outbox-producer";
      "outbox-delivery-state-machine";
      "runtime-config";
      "downstream-event-sinks";
      "compatibility-ledger-enforcement";
    ]);
    ("level", [ "tenant"; "application"; "product"; "product-user"; "product-user-group" ]);
    ("mapping", [
      "product-access-policy";
      "identity-binding-state-machine";
      "token-claim-contract";
      "admin-capability-matrix";
    ]);
    ("compat", [ "tenant-app-id"; "client-user-access"; "admin-master-key"; "legacy-callback-domain" ]);
  ]
  |> List.iter (fun (named, ids) -> require_ids dir all_forms named ids |> List.iter add);
  parsed_files
  |> List.iter (fun (path, forms) ->
         forms
         |> List.iter (fun root ->
                collect_forms "function" root
                |> List.iter (fun form -> validate_auth_structured_form path "function" form |> List.iter add);
                collect_forms "level" root
                |> List.iter (fun form -> validate_auth_structured_form path "level" form |> List.iter add);
                collect_forms "mapping" root
                |> List.iter (fun form -> validate_auth_structured_form path "mapping" form |> List.iter add)));
  List.rev !diagnostics

let validate_auth_domain file =
  try
    validate_auth_domain_source file (read_file file)
  with Sys_error msg -> [ diag file (synthetic_loc file) "io.error" msg ]

let validate_auth_domain_dir dir =
  try
    let files = lisp_files_under dir |> List.sort String.compare in
    let source = files |> List.map read_file |> String.concat "\n" in
    validate_auth_domain_source dir source @ validate_auth_domain_structured_dir dir
  with Sys_error msg -> [ diag dir (synthetic_loc dir) "io.error" msg ]

let m6_depth_required_concepts =
  [
    ("project.domain_model", [ "domain"; "authority"; "business" ]);
    ("project.policy_layer", [ "policy"; "guard"; "permission"; "capability" ]);
    ("project.flow_layer", [ "flow"; "state-machine"; "state machine"; "workflow" ]);
    ("project.event_contract", [ "event"; "outbox"; "event-bus"; "event bus" ]);
    ("project.runtime_projection", [ "runtime-projection"; "runtime projection"; "runtime" ]);
    ("project.implementation_map", [ "implementation"; "surface"; "code-isomorphism"; "current-code" ]);
    ("project.compatibility_ledger", [ "compatibility"; "ledger"; "legacy"; "bridge" ]);
    ("project.hot_path_wiring", [ "hot-path"; "hot path"; "runtime caller"; "runtime-callers" ]);
    ("project.regression_matrix", [ "regression-matrix"; "regression matrix"; "regression-tests"; "regression tests"; "backward compatibility" ]);
    ("project.final_m6_report", [ "final-m6-report"; "auth-grade"; "final-hardening-report"; "domain-hardening-report"; "final convergence"; "final-convergence" ]);
    ("project.behavior_closure", [ "behavior-universe"; "program-level behavior closure"; "observed behavior" ]);
  ]

let contains_any source needles =
  List.exists (contains_substring source) needles

let validate_m6_depth_source file source =
  m6_depth_required_concepts
  |> List.filter_map (fun (code, needles) ->
         if contains_any source needles then None
         else
           Some
             (diag file (synthetic_loc file) code
                (Printf.sprintf "project M6 depth evidence missing one of: %s"
                   (String.concat ", " needles))))

let validate_m6_depth_dir dir =
  try
    let files = lisp_files_under dir |> List.sort String.compare in
    let source = files |> List.map read_file |> String.concat "\n" in
    let diagnostics = ref [] in
    let add d = diagnostics := d :: !diagnostics in
    if files = [] then
      add (diag dir (synthetic_loc dir) "project.m6_lisp_missing"
             "project .missiond directory has no Lisp files for M6 depth");
    validate_m6_depth_source dir source |> List.iter add;
    validate_project_dir dir |> List.iter add;
    List.rev !diagnostics
  with Sys_error msg -> [ diag dir (synthetic_loc dir) "io.error" msg ]

let validate_domain_hardening_source = validate_m6_depth_source

let validate_domain_hardening_dir = validate_m6_depth_dir
