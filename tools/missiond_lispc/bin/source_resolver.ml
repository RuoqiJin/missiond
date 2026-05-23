open Ast

type source_unit = {
  file : string;
  kind : string;
  domain_id : string;
  included_by : string option;
  include_line : int option;
  source_hash : string;
}

type source_domain = {
  id : string;
  source_hash : string;
  source_units : source_unit list;
}

type resolved = {
  forms : sexp list;
  root : sexp option;
  source_hash : string;
  source_units : source_unit list;
  source_domains : source_domain list;
}

let is_absolute path =
  String.length path > 0 && (path.[0] = '/' || path.[0] = '~')

let has_parent_segment path =
  path |> String.split_on_char '/' |> List.exists (( = ) "..")

let valid_include_path path =
  starts_with ~prefix:"shards/" path
  && (not (is_absolute path))
  && not (has_parent_segment path)

let valid_shard_index_path path = path = "shards/index.lisp"

let include_target = function
  | List (_, _, [ Atom (_, "include"); String (_, target) ]) -> Some target
  | List (_, _, [ Atom (_, "include"); Atom (_, target) ]) -> Some target
  | _ -> None

let include_shard_index_target = function
  | List (_, _, [ Atom (_, "include-shard-index"); String (_, target) ]) ->
      Some target
  | List (_, _, [ Atom (_, "include-shard-index"); Atom (_, target) ]) ->
      Some target
  | _ -> None

let section_kind form = Option.value ~default:"<unknown>" (head form)

let source_unit ?included_by ?include_line ?(domain_id = "blueprint-core") file
    source kind =
  { file; kind; domain_id; included_by; include_line; source_hash = source_hash source }

let source_unit_to_json unit =
  Printf.sprintf
    {|{"file":%s,"kind":%s,"included_by":%s,"include_line":%s,"source_hash":%s}|}
    (json_string unit.file)
    (json_string unit.kind)
    (match unit.included_by with Some v -> json_string v | None -> "null")
    (match unit.include_line with Some v -> string_of_int v | None -> "null")
    (json_string unit.source_hash)

let source_units_to_json units =
  "[" ^ (units |> List.map source_unit_to_json |> String.concat ",") ^ "]"

let source_domain_to_json domain =
  Printf.sprintf {|{"id":%s,"source_hash":%s,"source_units":%s}|}
    (json_string domain.id)
    (json_string domain.source_hash)
    (source_units_to_json domain.source_units)

let source_domains_to_json domains =
  "["
  ^ (domains |> List.map source_domain_to_json |> String.concat ",")
  ^ "]"

let source_domains_to_json_for_ids domains ids =
  domains
  |> List.filter (fun domain -> List.mem domain.id ids)
  |> source_domains_to_json

let valid_domain_id id =
  let len = String.length id in
  len > 0
  &&
  let rec loop idx =
    if idx >= len then true
    else
      match id.[idx] with
      | 'a' .. 'z' | '0' .. '9' | '-' -> loop (idx + 1)
      | _ -> false
  in
  loop 0

let unique_domain_ids units =
  let rec loop seen out = function
    | [] -> List.rev out
    | unit :: rest ->
        if List.mem unit.domain_id seen then loop seen out rest
        else loop (unit.domain_id :: seen) (unit.domain_id :: out) rest
  in
  loop [] [] units

let source_domains_of_units units =
  unique_domain_ids units
  |> List.map (fun id ->
         let source_units =
           units |> List.filter (fun unit -> unit.domain_id = id)
         in
         let source_hash =
           source_units
           |> List.map (fun (unit : source_unit) -> unit.source_hash)
           |> String.concat "\n"
           |> source_hash
         in
         { id; source_hash; source_units })

let shard_kind forms =
  match forms with
  | [ form ] -> section_kind form
  | _ -> "multi-section-shard"

let merge_section name forms =
  let sections = forms |> List.filter (fun form -> is_list form name) in
  match sections with
  | [] | [ _ ] -> forms
  | first :: _ ->
      let merged_children =
        match children first with
        | [] -> []
        | head :: _ ->
            head
            :: (sections
               |> List.map (fun section ->
                      match children section with _ :: tail -> tail | [] -> [])
               |> List.flatten)
      in
      let merged =
        match first with
        | List (loc, kind, _) -> List (loc, kind, merged_children)
        | _ -> first
      in
      let inserted = ref false in
      forms
      |> List.filter_map (fun form ->
             if is_list form name then
               if !inserted then None
               else (
                 inserted := true;
                 Some merged)
             else Some form)

let merge_compiler_sections forms =
  forms |> merge_section "implementation-map"

let load_shard ~blueprint_dir ~included_by ~include_loc ~stack ~domain_id target =
  if not (valid_include_path target) then
    raise
      (Reader_error
         ( include_loc,
           "include path must be relative to shards/, must not be absolute, and must not contain .."
         ));
  let file = Filename.concat blueprint_dir target in
  if List.mem file stack then
    raise
      (Reader_error
         ( include_loc,
           "include cycle detected: " ^ String.concat " -> " (List.rev (file :: stack))
         ));
  let source = read_file file in
  let forms = Parser.parse_source file source in
  match forms with
  | _ :: _ ->
      List.iter
        (fun form ->
          if is_list form "include"
             || is_list form "include-shard-index"
             || is_list form "missiond-blueprint"
          then
            raise
              (Reader_error
                 ( loc_of form,
                   "included shard must provide only blueprint sections; nested include and missiond-blueprint roots are forbidden"
                 )))
        forms;
      ( forms,
        source,
        source_unit ~included_by ~include_line:include_loc.line file source
          ~domain_id (shard_kind forms) )
  | [] ->
      raise (Reader_error (include_loc, "included shard is empty: " ^ target))

let compiler_active_shards index_file forms =
  let root =
    match find_root forms "missiond-blueprint-shards" with
    | Some root -> root
    | None ->
        raise
          (Reader_error
             ( synthetic_loc index_file,
               "shard index must contain a missiond-blueprint-shards root" ))
  in
  children root
  |> List.filter (fun node -> is_list node "shard")
  |> List.filter_map (fun shard ->
         let props = keyword_props ~start:2 shard in
         match (prop_text ":status" props, prop_text ":path" props) with
         | Some "compiler-active", Some path -> (
             match prop_text ":domain" props with
             | Some domain when valid_domain_id domain ->
                 Some (path, domain, loc_of shard)
             | Some domain ->
                 raise
                   (Reader_error
                      ( loc_of shard,
                        "compiler-active shard declares invalid :domain "
                        ^ domain ))
             | None ->
                 raise
                   (Reader_error
                      ( loc_of shard,
                        "compiler-active shard must declare :domain; source domains are SSOT data, not compiler path inference"
                      )))
         | _ -> None)

let load_shard_index ~blueprint_dir ~included_by ~include_loc ~stack target =
  if not (valid_shard_index_path target) then
    raise
      (Reader_error
         ( include_loc,
           "include-shard-index path must be exactly shards/index.lisp" ));
  let index_file = Filename.concat blueprint_dir target in
  if List.mem index_file stack then
    raise
      (Reader_error
         ( include_loc,
           "include cycle detected: "
           ^ String.concat " -> " (List.rev (index_file :: stack)) ));
  let index_source = read_file index_file in
  let index_forms = Parser.parse_source index_file index_source in
  let index_unit =
    source_unit ~included_by ~include_line:include_loc.line index_file
      index_source "missiond-blueprint-shards"
  in
  let loaded =
    compiler_active_shards index_file index_forms
    |> List.map (fun (target, domain_id, shard_loc) ->
           let forms, source, unit =
             load_shard ~blueprint_dir ~included_by:index_file
               ~include_loc:shard_loc ~stack:(index_file :: stack) ~domain_id
               target
           in
           (forms, source, unit))
  in
  let forms = loaded |> List.map (fun (forms, _, _) -> forms) |> List.flatten in
  let units = index_unit :: (loaded |> List.map (fun (_, _, unit) -> unit)) in
  if forms = [] then
    raise
      (Reader_error
         (include_loc, "include-shard-index did not resolve any compiler-active shards"));
  (forms, index_source, units)

let expand_root blueprint_file root =
  let blueprint_dir = Filename.dirname blueprint_file in
  let root_loc = loc_of root in
  match root with
  | List (loc, kind, xs) ->
      let units = ref [] in
      let expanded =
        xs
        |> List.map (fun child ->
               match include_target child with
               | None -> (
                   match include_shard_index_target child with
                   | None -> [ child ]
                   | Some target ->
                       let forms, _source, loaded_units =
                         load_shard_index ~blueprint_dir ~included_by:blueprint_file
                           ~include_loc:(loc_of child) ~stack:[ blueprint_file ] target
                       in
                       units := List.rev loaded_units @ !units;
                       forms)
               | Some target ->
                   let forms, _source, unit =
                     load_shard ~blueprint_dir ~included_by:blueprint_file
                       ~include_loc:(loc_of child) ~stack:[ blueprint_file ]
                       ~domain_id:"blueprint-core" target
                   in
                   units := unit :: !units;
                   forms)
        |> List.flatten
        |> merge_compiler_sections
      in
      (List (loc, kind, expanded), List.rev !units)
  | _ ->
      raise (Reader_error (root_loc, "missiond-blueprint root must be a list"))

let resolve_blueprint_file blueprint_file =
  let root_source = read_file blueprint_file in
  let forms = Parser.parse_source blueprint_file root_source in
  let root = find_root forms "missiond-blueprint" in
  let root_unit =
    source_unit blueprint_file root_source
      (match root with Some _ -> "missiond-blueprint" | None -> "<missing>")
  in
  match root with
  | None ->
      {
        forms;
        root = None;
        source_hash = source_hash root_source;
        source_units = [ root_unit ];
        source_domains = source_domains_of_units [ root_unit ];
      }
  | Some root ->
      let expanded_root, shard_units = expand_root blueprint_file root in
      let resolved_forms =
        forms
        |> List.map (fun form ->
               if is_list form "missiond-blueprint" then expanded_root else form)
      in
      let source_units = root_unit :: shard_units in
      let composite_hash =
        source_units
        |> List.map (fun (unit : source_unit) -> unit.source_hash)
        |> String.concat "\n"
        |> source_hash
      in
      {
        forms = resolved_forms;
        root = Some expanded_root;
        source_hash = composite_hash;
        source_units;
        source_domains = source_domains_of_units source_units;
      }
