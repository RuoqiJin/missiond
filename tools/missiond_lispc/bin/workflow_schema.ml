open Ast

let validate file =
  try
    let forms = Parser.parse_file file in
    let diagnostics = ref [] in
    let add d = diagnostics := d :: !diagnostics in
    let workflow = List.find_opt (fun n -> is_list n "workflow") forms in
    (match workflow with
    | None -> add (diag file { line = 1; column = 1 } "workflow.missing" "missing workflow root")
    | Some wf ->
        let props = keyword_props ~start:1 wf in
        if prop_text ":workflow_id" props = None then
          add (diag file (loc_of wf) "workflow.workflow_id_missing" "missing :workflow_id");
        if prop_text ":status" props = None then
          add (diag file (loc_of wf) "workflow.status_missing" "missing :status");
        let text = read_file file in
        let required = [ ":source_plans"; ":steps"; ":risk-gates"; ":completion" ] in
        List.iter
          (fun needle ->
            if not (contains_substring text needle) then
              add (diag file (loc_of wf) "workflow.required_token_missing"
                     (Printf.sprintf "missing workflow token %s" needle)))
          required;
        let steps =
          let rec collect acc = function
            | List (_, _, xs) as n ->
                let acc = if is_list n "step" then n :: acc else acc in
                List.fold_left collect acc xs
            | _ -> acc
          in
          collect [] wf
        in
        if steps = [] then add (diag file (loc_of wf) "workflow.steps_empty" "workflow has no steps"));
    List.rev !diagnostics
  with
  | Reader_error (l, msg) -> [ diag file l "parse.error" msg ]
  | Sys_error msg -> [ diag file { line = 1; column = 1 } "io.error" msg ]
