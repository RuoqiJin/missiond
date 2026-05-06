type loc = { line : int; column : int }

type sexp =
  | Atom of loc * string
  | String of loc * string
  | List of loc * list_kind * sexp list

and list_kind = Paren | Bracket

type diagnostic = {
  file : string;
  line : int;
  column : int;
  code : string;
  message : string;
  path : string;
}

exception Reader_error of loc * string

let loc_of = function
  | Atom (loc, _) -> loc
  | String (loc, _) -> loc
  | List (loc, _, _) -> loc

let json_escape s =
  let b = Buffer.create (String.length s + 16) in
  String.iter
    (function
      | '"' -> Buffer.add_string b "\\\""
      | '\\' -> Buffer.add_string b "\\\\"
      | '\n' -> Buffer.add_string b "\\n"
      | '\r' -> Buffer.add_string b "\\r"
      | '\t' -> Buffer.add_string b "\\t"
      | c when Char.code c < 0x20 ->
          Buffer.add_string b (Printf.sprintf "\\u%04x" (Char.code c))
      | c -> Buffer.add_char b c)
    s;
  Buffer.contents b

let json_string s = "\"" ^ json_escape s ^ "\""

let diagnostic_to_json d =
  Printf.sprintf
    {|{"file":%s,"line":%d,"column":%d,"code":%s,"message":%s,"path":%s}|}
    (json_string d.file) d.line d.column (json_string d.code)
    (json_string d.message) (json_string d.path)

let result_json ?(extra = []) ok diagnostics =
  let diag_json =
    diagnostics |> List.map diagnostic_to_json |> String.concat ","
  in
  let extra_json =
    match extra with
    | [] -> ""
    | fields -> "," ^ String.concat "," fields
  in
  Printf.sprintf {|{"ok":%s,"diagnostics":[%s]%s}|}
    (if ok then "true" else "false")
    diag_json extra_json

let atom_text = function Atom (_, s) -> Some s | String (_, s) -> Some s | _ -> None

let head = function
  | List (_, _, Atom (_, h) :: _) -> Some h
  | _ -> None

let children = function List (_, _, xs) -> xs | _ -> []

let is_list node expected = match head node with Some h -> h = expected | None -> false

let list_texts = function
  | List (_, _, xs) -> List.filter_map atom_text xs
  | _ -> []

let starts_with ~prefix s =
  let lp = String.length prefix in
  String.length s >= lp && String.sub s 0 lp = prefix

let contains_substring text needle =
  let lt = String.length text in
  let ln = String.length needle in
  if ln = 0 then true
  else
    let rec loop i =
      if i + ln > lt then false
      else if String.sub text i ln = needle then true
      else loop (i + 1)
    in
    loop 0

let read_file file =
  let ic = open_in_bin file in
  Fun.protect
    ~finally:(fun () -> close_in_noerr ic)
    (fun () ->
      let len = in_channel_length ic in
      really_input_string ic len)

module Parser = struct
  type t = {
    source : string;
    mutable i : int;
    mutable line : int;
    mutable column : int;
  }

  let make _file source = { source; i = 0; line = 1; column = 1 }
  let eof p = p.i >= String.length p.source
  let peek p = if eof p then '\000' else p.source.[p.i]
  let loc p = { line = p.line; column = p.column }

  let advance p =
    let c = peek p in
    p.i <- p.i + 1;
    if c = '\n' then (
      p.line <- p.line + 1;
      p.column <- 1)
    else p.column <- p.column + 1;
    c

  let fail p msg = raise (Reader_error (loc p, msg))

  let rec skip_space_and_comments p =
    if not (eof p) then
      match peek p with
      | (' ' | '\n' | '\r' | '\t') ->
          ignore (advance p);
          skip_space_and_comments p
      | ';' ->
          while (not (eof p)) && peek p <> '\n' do
            ignore (advance p)
          done;
          skip_space_and_comments p
      | _ -> ()

  let rec parse_forms p close =
    skip_space_and_comments p;
    if eof p then (
      match close with
      | None -> []
      | Some c -> fail p (Printf.sprintf "missing closing delimiter '%c'" c))
    else
      match (close, peek p) with
      | Some c, got when got = c ->
          ignore (advance p);
          []
      | _, (')' | ']') ->
          fail p (Printf.sprintf "unexpected closing delimiter '%c'" (peek p))
      | _ ->
          let form = parse_form p in
          form :: parse_forms p close

  and parse_form p =
    skip_space_and_comments p;
    match peek p with
    | '(' -> parse_list p Paren ')'
    | '[' -> parse_list p Bracket ']'
    | '"' -> parse_string p
    | _ -> parse_atom p

  and parse_list p kind close =
    let l = loc p in
    ignore (advance p);
    let xs = parse_forms p (Some close) in
    List (l, kind, xs)

  and parse_string p =
    let l = loc p in
    ignore (advance p);
    let b = Buffer.create 32 in
    let rec loop () =
      if eof p then raise (Reader_error (l, "unterminated string"));
      match advance p with
      | '"' -> String (l, Buffer.contents b)
      | '\\' ->
          if eof p then raise (Reader_error (l, "unterminated string escape"));
          Buffer.add_char b (advance p);
          loop ()
      | c ->
          Buffer.add_char b c;
          loop ()
    in
    loop ()

  and parse_atom p =
    let l = loc p in
    let b = Buffer.create 16 in
    let rec loop () =
      if eof p then ()
      else
        match peek p with
        | (' ' | '\n' | '\r' | '\t' | '(' | ')' | '[' | ']' | ';') -> ()
        | c ->
            Buffer.add_char b c;
            ignore (advance p);
            loop ()
    in
    loop ();
    let value = Buffer.contents b in
    if value = "" then fail p (Printf.sprintf "unexpected character '%c'" (peek p));
    Atom (l, value)
end

let parse_file file =
  let source = read_file file in
  let p = Parser.make file source in
  Parser.parse_forms p None

let diag ?(path = "") (file : string) (loc : loc) code message =
  { file; line = loc.line; column = loc.column; code; message; path }

let find_root forms expected =
  List.find_opt (fun form -> is_list form expected) forms

let find_child node expected =
  children node |> List.find_opt (fun child -> is_list child expected)

let keyword_props ?(start = 1) node =
  let xs = children node in
  let rec loop idx acc =
    if idx >= List.length xs then List.rev acc
    else
      match List.nth xs idx with
      | Atom (_, key) when starts_with ~prefix:":" key ->
          let value =
            if idx + 1 < List.length xs then Some (List.nth xs (idx + 1)) else None
          in
          loop (idx + 2) ((key, value) :: acc)
      | _ -> loop (idx + 1) acc
  in
  loop start []

let prop key props = List.assoc_opt key props |> Option.join
let prop_text key props = match prop key props with Some node -> atom_text node | None -> None

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
               let errs =
                 match logic with
                 | Some s when String.trim s <> "" -> errs
                 | _ ->
                     diag file (loc_of step) "core.step_logic"
                       (Printf.sprintf "function %s step %s must declare :logic"
                          fn_id expected)
                     :: errs
               in
               errs)
        |> List.flatten
  | Some node ->
      [ diag file (loc_of node) "core.invalid"
          (Printf.sprintf "function %s must declare list :core" fn_id)
      ]
  | None ->
      [ diag file { line = 1; column = 1 } "core.missing"
          (Printf.sprintf "function %s must declare :core" fn_id)
      ]

let validate_v3 file expected_surfaces =
  try
    let forms = parse_file file in
    let diagnostics = ref [] in
    let root = find_root forms "missiond-blueprint" in
    let add d = diagnostics := d :: !diagnostics in
    (match root with
    | None -> add (diag file { line = 1; column = 1 } "root.missing" "missing missiond-blueprint root")
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
    let ds = List.rev !diagnostics in
    print_endline (result_json (ds = []) ds);
    if ds = [] then 0 else 1
  with
  | Reader_error (l, msg) ->
      let d = diag file l "parse.error" msg in
      print_endline (result_json false [ d ]);
      1
  | Sys_error msg ->
      let d = diag file { line = 1; column = 1 } "io.error" msg in
      print_endline (result_json false [ d ]);
      1

let validate_workflow file =
  try
    let forms = parse_file file in
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
    let ds = List.rev !diagnostics in
    print_endline (result_json (ds = []) ds);
    if ds = [] then 0 else 1
  with
  | Reader_error (l, msg) ->
      let d = diag file l "parse.error" msg in
      print_endline (result_json false [ d ]);
      1
  | Sys_error msg ->
      let d = diag file { line = 1; column = 1 } "io.error" msg in
      print_endline (result_json false [ d ]);
      1

let validate_project file =
  try
    let forms = parse_file file in
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
    let ds = List.rev !diagnostics in
    print_endline (result_json (ds = []) ds);
    if ds = [] then 0 else 1
  with
  | Reader_error (l, msg) ->
      let d = diag file l "parse.error" msg in
      print_endline (result_json false [ d ]);
      1
  | Sys_error msg ->
      let d = diag file { line = 1; column = 1 } "io.error" msg in
      print_endline (result_json false [ d ]);
      1

let emit_json file =
  let rec sexp_to_json = function
    | Atom (l, value) ->
        Printf.sprintf {|{"type":"atom","value":%s,"line":%d,"column":%d}|}
          (json_string value) l.line l.column
    | String (l, value) ->
        Printf.sprintf {|{"type":"string","value":%s,"line":%d,"column":%d}|}
          (json_string value) l.line l.column
    | List (l, kind, xs) ->
        let kind = match kind with Paren -> "paren" | Bracket -> "bracket" in
        Printf.sprintf {|{"type":"list","kind":%s,"line":%d,"column":%d,"children":[%s]}|}
          (json_string kind) l.line l.column
          (xs |> List.map sexp_to_json |> String.concat ",")
  in
  try
    let forms = parse_file file in
    Printf.printf {|{"ok":true,"forms":[%s]}%s|}
      (forms |> List.map sexp_to_json |> String.concat ",")
      "\n";
    0
  with Reader_error (l, msg) ->
    let d = diag file l "parse.error" msg in
    print_endline (result_json false [ d ]);
    1

let find_arg name args =
  let rec loop = function
    | [] -> None
    | x :: y :: _ when x = name -> Some y
    | x :: _ when starts_with ~prefix:(name ^ "=") x ->
        Some (String.sub x (String.length name + 1) (String.length x - String.length name - 1))
    | _ :: xs -> loop xs
  in
  loop args

let has_flag flag args = List.exists (( = ) flag) args

let split_csv s =
  s |> String.split_on_char ',' |> List.map String.trim |> List.filter (( <> ) "")

let usage () =
  prerr_endline
    "Usage: missiond-lispc <emit-json|check-v3|check-workflow|check-project> --file <path>|--blueprint <path>";
  2

let () =
  let args = Array.to_list Sys.argv |> List.tl in
  match args with
  | "emit-json" :: rest -> (
      match find_arg "--file" rest with Some file -> exit (emit_json file) | None -> exit (usage ()))
  | "check-v3" :: rest -> (
      let file = Option.value ~default:".missiond/v3/missiond-blueprint.lisp" (find_arg "--blueprint" rest) in
      let expected =
        find_arg "--expected-surfaces" rest |> Option.map split_csv |> Option.value ~default:[]
      in
      exit (validate_v3 file expected))
  | "check-workflow" :: rest -> (
      match find_arg "--file" rest with Some file -> exit (validate_workflow file) | None -> exit (usage ()))
  | "check-project" :: rest -> (
      match find_arg "--file" rest with
      | Some file -> exit (validate_project file)
      | None -> (
          match find_arg "--blueprint" rest with
          | Some file -> exit (validate_project file)
          | None -> exit (usage ())))
  | _ when has_flag "--help" args -> exit (usage ())
  | _ -> exit (usage ())
