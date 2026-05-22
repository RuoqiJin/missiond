type loc = { source_file : string; line : int; column : int }

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

let read_file file =
  let ic = open_in_bin file in
  Fun.protect
    ~finally:(fun () -> close_in_noerr ic)
    (fun () ->
      let len = in_channel_length ic in
      really_input_string ic len)

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

let synthetic_loc file = { source_file = file; line = 1; column = 1 }

let diag ?(path = "") (file : string) (loc : loc) code message =
  let file = if loc.source_file = "" then file else loc.source_file in
  { file; line = loc.line; column = loc.column; code; message; path }

let atom_text = function
  | Atom (_, s) -> Some s
  | String (_, s) -> Some s
  | _ -> None

let head = function
  | List (_, _, Atom (_, h) :: _) -> Some h
  | _ -> None

let children = function
  | List (_, _, xs) -> xs
  | _ -> []

let is_list node expected =
  match head node with
  | Some h -> h = expected
  | None -> false

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

let prop_text key props =
  match prop key props with
  | Some node -> atom_text node
  | None -> None

let sexp_to_json =
  let rec loop = function
    | Atom (l, value) ->
        Printf.sprintf
          {|{"type":"atom","value":%s,"file":%s,"line":%d,"column":%d}|}
          (json_string value) (json_string l.source_file) l.line l.column
    | String (l, value) ->
        Printf.sprintf
          {|{"type":"string","value":%s,"file":%s,"line":%d,"column":%d}|}
          (json_string value) (json_string l.source_file) l.line l.column
    | List (l, kind, xs) ->
        let kind = match kind with Paren -> "paren" | Bracket -> "bracket" in
        Printf.sprintf
          {|{"type":"list","kind":%s,"file":%s,"line":%d,"column":%d,"children":[%s]}|}
          (json_string kind) (json_string l.source_file) l.line l.column
          (xs |> List.map loop |> String.concat ",")
  in
  loop

let lisp_string s =
  let b = Buffer.create (String.length s + 16) in
  Buffer.add_char b '"';
  String.iter
    (function
      | '"' -> Buffer.add_string b "\\\""
      | '\\' -> Buffer.add_string b "\\\\"
      | '\n' -> Buffer.add_string b "\\n"
      | '\r' -> Buffer.add_string b "\\r"
      | '\t' -> Buffer.add_string b "\\t"
      | c -> Buffer.add_char b c)
    s;
  Buffer.add_char b '"';
  Buffer.contents b

let sexp_to_lisp =
  let rec loop = function
    | Atom (_, value) -> value
    | String (_, value) -> lisp_string value
    | List (_, kind, xs) ->
        let open_delim, close_delim =
          match kind with Paren -> ("(", ")") | Bracket -> ("[", "]")
        in
        open_delim ^ (xs |> List.map loop |> String.concat " ") ^ close_delim
  in
  loop

let source_hash s = Digest.to_hex (Digest.string s)
