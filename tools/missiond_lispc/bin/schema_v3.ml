open Ast

let surface_ids implementation_map =
  children implementation_map
  |> List.filter (fun n -> is_list n "surface")
  |> List.filter_map (fun n ->
         match children n with
         | _ :: id :: _ -> atom_text id
         | _ -> None)

let validate_core_steps file fn_id core =
  match core with
  | Some (List (_, _, xs) as core_node) ->
      let steps = List.filter (fun n -> is_list n "step") xs in
      if steps = [] then
        [ (diag file (loc_of core_node) "core.empty"
             (Printf.sprintf "function %s :core must contain at least one step" fn_id)
          )
        ]
      else
        steps
        |> List.mapi (fun i step ->
               let expected = "s" ^ string_of_int (i + 1) in
               let got =
                 match children step with _ :: id :: _ -> atom_text id | _ -> None
               in
               let props = keyword_props ~start:2 step in
               let logic = prop_text ":logic" props in
               let errs =
                 if got <> Some expected then
                   [ diag file (loc_of step) "core.step_order"
                       (Printf.sprintf "function %s step %d must be %s" fn_id
                          (i + 1) expected)
                   ]
                 else []
               in
               match logic with
               | Some s when String.trim s <> "" -> errs
               | _ ->
                   diag file (loc_of step) "core.step_logic"
                     (Printf.sprintf "function %s step %s must declare :logic"
                        fn_id expected)
                   :: errs)
        |> List.flatten
  | Some node ->
      [ diag file (loc_of node) "core.invalid"
          (Printf.sprintf "function %s must declare list :core" fn_id)
      ]
  | None ->
      [ diag file (synthetic_loc file) "core.missing"
          (Printf.sprintf "function %s must declare :core" fn_id)
      ]

let rec collect_named_forms name node =
  let here = if is_list node name then [ node ] else [] in
  match node with
  | List (_, _, xs) -> here @ (xs |> List.map (collect_named_forms name) |> List.flatten)
  | _ -> here

let validate_policy_clauses file root =
  let diagnostics = ref [] in
  let seen = Hashtbl.create 64 in
  let add d = diagnostics := d :: !diagnostics in
  collect_named_forms "policy-clause" root
  |> List.iter (fun clause ->
         let id =
           match children clause with _ :: id :: _ -> atom_text id | _ -> None
         in
         let label = Option.value ~default:"<missing>" id in
         (match id with
         | Some value when String.trim value <> "" ->
             if Hashtbl.mem seen value then
               add
                 (diag file (loc_of clause) "policy_clause.duplicate_id"
                    (Printf.sprintf "duplicate policy-clause id %s" value))
             else Hashtbl.add seen value true
         | _ ->
             add
               (diag file (loc_of clause) "policy_clause.id_missing"
                  "policy-clause must declare an id"));
         let props = keyword_props ~start:2 clause in
         (match prop_text ":owner" props with
         | Some value when String.trim value <> "" -> ()
         | _ ->
             add
               (diag file (loc_of clause) "policy_clause.owner_missing"
                  (Printf.sprintf "policy-clause %s missing :owner" label)));
         let require_non_empty_list key code =
           match prop key props |> Option.map list_texts with
           | Some (_ :: _) -> ()
           | _ ->
               add
                 (diag file (loc_of clause) code
                    (Printf.sprintf "policy-clause %s missing non-empty %s" label key))
         in
         require_non_empty_list ":applies-to" "policy_clause.applies_to_missing";
         require_non_empty_list ":must" "policy_clause.must_missing");
  List.rev !diagnostics

let validate file expected_surfaces =
  try
    let resolved = Source_resolver.resolve_blueprint_file file in
    let forms = resolved.forms in
    let diagnostics = ref [] in
    let root =
      match resolved.root with
      | Some root -> Some root
      | None -> find_root forms "missiond-blueprint"
    in
    let add d = diagnostics := d :: !diagnostics in
    (match root with
    | None -> add (diag file (synthetic_loc file) "root.missing" "missing missiond-blueprint root")
    | Some root -> (
        let implementation_map = find_child root "implementation-map" in
        let flow_map = find_child root "pillar-flow-map" in
        let surfaces =
          match implementation_map with Some m -> surface_ids m | None -> []
        in
        if implementation_map = None then
          add (diag file (loc_of root) "implementation_map.missing" "missing implementation-map");
        if flow_map = None then
          add (diag file (loc_of root) "pillar_flow_map.missing" "missing pillar-flow-map");
        validate_policy_clauses file root |> List.iter add;
        List.iter
          (fun expected ->
            if not (List.mem expected surfaces) then
              add
                (diag file (loc_of root) "surface.missing"
                   (Printf.sprintf "missing implementation surface %s" expected)))
          expected_surfaces;
        (match flow_map with
        | None -> ()
        | Some fm ->
            let mapped = Hashtbl.create 64 in
            children fm
            |> List.filter (fun n -> is_list n "pillar")
            |> List.iter (fun pillar ->
                   children pillar
                   |> List.filter (fun n -> is_list n "function")
                   |> List.iter (fun fn ->
                          let fn_id =
                            match children fn with
                            | _ :: id :: _ -> Option.value ~default:"<missing>" (atom_text id)
                            | _ -> "<missing>"
                          in
                          let props = keyword_props ~start:2 fn in
                          let surface = prop_text ":surface" props in
                          let entry = prop ":entry" props |> Option.map list_texts |> Option.value ~default:[] in
                          let egress = prop ":egress" props |> Option.map list_texts |> Option.value ~default:[] in
                          if entry = [] then
                            add (diag file (loc_of fn) "function.entry_missing"
                                   (Printf.sprintf "function %s missing :entry" fn_id));
                          if egress = [] then
                            add (diag file (loc_of fn) "function.egress_missing"
                                   (Printf.sprintf "function %s missing :egress" fn_id));
                          validate_core_steps file fn_id (prop ":core" props)
                          |> List.iter add;
                          match surface with
                          | None -> add (diag file (loc_of fn) "function.surface_missing"
                                           (Printf.sprintf "function %s missing :surface" fn_id))
                          | Some s ->
                              let current = Option.value ~default:0 (Hashtbl.find_opt mapped s) in
                              Hashtbl.replace mapped s (current + 1)));
            List.iter
              (fun expected ->
                let count = Option.value ~default:0 (Hashtbl.find_opt mapped expected) in
                if count = 0 then
                  add (diag file (loc_of fm) "surface.unmapped"
                         (Printf.sprintf "surface %s is not mapped by pillar-flow-map" expected))
                else if count > 1 then
                  add (diag file (loc_of fm) "surface.duplicate_mapping"
                         (Printf.sprintf "surface %s is mapped by multiple functions" expected)))
              expected_surfaces)));
    List.rev !diagnostics
  with
  | Reader_error (l, msg) -> [ diag file l "parse.error" msg ]
  | Sys_error msg -> [ diag file (synthetic_loc file) "io.error" msg ]
