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

let validate_project_core_steps file form_id_label form =
  let props = keyword_props ~start:2 form in
  match prop ":core" props with
  | Some (List (_, _, xs) as core) ->
      let steps = xs |> List.filter (fun node -> is_list node "step") in
      if steps = [] then
        [ diag file (loc_of core) "project.core_empty"
            (Printf.sprintf "%s :core must contain at least one ordered step" form_id_label)
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
        |> List.flatten
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
  if prop ":entry" props = None then
    add (diag file (loc_of form) "project.entry_missing" (label ^ " missing :entry"));
  validate_project_core_steps file label form |> List.iter add;
  if prop ":egress" props = None then
    add (diag file (loc_of form) "project.egress_missing" (label ^ " missing :egress"));
  if prop ":surface" props = None && not (nonempty_list_prop ":surfaces" props) then
    add (diag file (loc_of form) "project.surface_missing" (label ^ " missing :surface or non-empty :surfaces"));
  List.rev !diagnostics

let validate file =
  try
    let forms = Parser.parse_file file in
    let diagnostics = ref [] in
    let add d = diagnostics := d :: !diagnostics in
    (match find_root forms "missiond-blueprint" with
    | None ->
        if forms = [] then
          add (diag file { line = 1; column = 1 } "project.root_missing" "missing Lisp root");
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
            add (diag file { line = 1; column = 1 } "project.function_missing"
                   "project blueprint/intent must declare at least one function")
    | Some root ->
        if find_child root "project-maturity-registry" = None then
          add (diag file (loc_of root) "project.maturity_missing" "missing project-maturity-registry");
        if find_child root "project-blueprint-registry" = None then
          add (diag file (loc_of root) "project.registry_missing" "missing project-blueprint-registry"));
    List.rev !diagnostics
  with
  | Reader_error (l, msg) -> [ diag file l "parse.error" msg ]
  | Sys_error msg -> [ diag file { line = 1; column = 1 } "io.error" msg ]

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
             (diag file { line = 1; column = 1 } code
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
             (diag file { line = 1; column = 1 }
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
             add (diag path { line = 1; column = 1 } "auth.shard_missing" "missing required auth shard");
             None)
           else
             try Some (path, Parser.parse_file path)
             with
             | Reader_error (loc, msg) ->
                 add (diag path loc "parse.error" msg);
                 None
             | Sys_error msg ->
                 add (diag path { line = 1; column = 1 } "io.error" msg);
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
  with Sys_error msg -> [ diag file { line = 1; column = 1 } "io.error" msg ]

let rec lisp_files_under dir =
  Sys.readdir dir
  |> Array.to_list
  |> List.map (Filename.concat dir)
  |> List.map (fun path ->
         if Sys.is_directory path then lisp_files_under path
         else if Filename.check_suffix path ".lisp" then [ path ]
         else [])
  |> List.flatten

let validate_auth_domain_dir dir =
  try
    let files = lisp_files_under dir |> List.sort String.compare in
    let source = files |> List.map read_file |> String.concat "\n" in
    validate_auth_domain_source dir source @ validate_auth_domain_structured_dir dir
  with Sys_error msg -> [ diag dir { line = 1; column = 1 } "io.error" msg ]
