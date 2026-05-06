open Ast

let validate file =
  try
    let forms = Parser.parse_file file in
    let diagnostics = ref [] in
    let add d = diagnostics := d :: !diagnostics in
    let text = read_file file in
    (match find_root forms "missiond-blueprint" with
    | None ->
        if forms = [] then
          add (diag file { line = 1; column = 1 } "project.root_missing" "missing Lisp root");
        if contains_substring text "blueprint" || contains_substring text "(function" then (
          if not (contains_substring text ":entry") then
            add (diag file { line = 1; column = 1 } "project.entry_missing" "project blueprint/function text missing :entry");
          if not (contains_substring text ":core") then
            add (diag file { line = 1; column = 1 } "project.core_missing" "project blueprint/function text missing :core");
          if not (contains_substring text ":egress") then
            add (diag file { line = 1; column = 1 } "project.egress_missing" "project blueprint/function text missing :egress"))
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
    validate_auth_domain_source dir source
  with Sys_error msg -> [ diag dir { line = 1; column = 1 } "io.error" msg ]
