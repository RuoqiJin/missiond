open Ast

type molecule = {
  id : string;
  on : string;
  when_ : string option;
  atoms : string list;
  effects : string list;
}

type tissue = {
  id : string;
  receptors : string list;
  allow_atoms : string list;
  allow_effects : string list;
  budgets : (string * string) list;
  molecules : molecule list;
}

type organ = { id : string; tissues : tissue list }

type genome = {
  file : string;
  id : string;
  schema : string;
  activation : string;
  organs : organ list;
}

let genome_files dir =
  Sys.readdir dir
  |> Array.to_list
  |> List.filter (fun name -> Filename.check_suffix name ".lisp")
  |> List.sort String.compare
  |> List.map (Filename.concat dir)

let prop_text_list key props =
  match prop key props with
  | Some value -> list_texts value
  | None -> []

let form_id = function
  | List (_, _, _ :: id_node :: _) -> atom_text id_node
  | _ -> None

let list_forms named node =
  children node |> List.filter (fun child -> is_list child named)

let starts_keyword node =
  match atom_text node with
  | Some value -> starts_with ~prefix:":" value
  | None -> false

let child_forms_after_props named node =
  children node
  |> List.filter (fun child -> (not (starts_keyword child)) && is_list child named)

let budget_entries node =
  match node with
  | List (_, _, xs) ->
      let rec loop acc = function
        | Atom (_, key) :: value :: rest when starts_with ~prefix:":" key -> (
            match atom_text value with
            | Some text -> loop ((key, text) :: acc) rest
            | None -> loop acc rest)
        | _ :: rest -> loop acc rest
        | [] -> List.rev acc
      in
      loop [] xs
  | _ -> []

let budget_number key entries default_value =
  List.assoc_opt key entries |> Option.value ~default:default_value

let molecule_of_form node =
  let props = keyword_props ~start:2 node in
  {
    id = form_id node |> Option.value ~default:"";
    on = prop_text ":on" props |> Option.value ~default:"";
    when_ = prop_text ":when" props;
    atoms = prop_text_list ":atoms" props;
    effects = prop_text_list ":effects" props;
  }

let tissue_of_form node =
  let props = keyword_props ~start:2 node in
  let budgets =
    match prop ":budgets" props with
    | Some value -> budget_entries value
    | None -> []
  in
  {
    id = form_id node |> Option.value ~default:"";
    receptors = prop_text_list ":receptors" props;
    allow_atoms = prop_text_list ":allow-atoms" props;
    allow_effects = prop_text_list ":allow-effects" props;
    budgets;
    molecules = child_forms_after_props "molecule" node |> List.map molecule_of_form;
  }

let organ_of_form node =
  {
    id = form_id node |> Option.value ~default:"";
    tissues = child_forms_after_props "tissue" node |> List.map tissue_of_form;
  }

let genome_of_root file root =
  let props = keyword_props ~start:2 root in
  {
    file;
    id = form_id root |> Option.value ~default:"";
    schema = prop_text ":schema" props |> Option.value ~default:"";
    activation = prop_text ":activation" props |> Option.value ~default:"";
    organs = child_forms_after_props "organ" root |> List.map organ_of_form;
  }

let list_contains value xs = List.exists (( = ) value) xs

let add_if condition diagnostic diagnostics =
  if condition then diagnostic :: diagnostics else diagnostics

let validate_molecule file tissue_node tissue molecule diagnostics =
  let loc = loc_of tissue_node in
  let diagnostics =
    diagnostics
    |> add_if (molecule.id = "") (diag file loc "genome.molecule_id_missing" "missing molecule id")
    |> add_if (molecule.on = "") (diag file loc "genome.molecule_on_missing" "missing molecule :on")
    |> add_if (molecule.effects = [])
         (diag file loc "genome.molecule_effects_missing" "missing molecule :effects")
  in
  let diagnostics =
    molecule.atoms
    |> List.fold_left
         (fun acc atom ->
           if list_contains atom tissue.allow_atoms then acc
           else
             diag file loc "genome.atom_not_allowed"
               (Printf.sprintf "molecule %s uses atom %s outside :allow-atoms" molecule.id atom)
             :: acc)
         diagnostics
  in
  molecule.effects
  |> List.fold_left
       (fun acc eff ->
         if list_contains eff tissue.allow_effects then acc
         else
           diag file loc "genome.effect_not_allowed"
             (Printf.sprintf "molecule %s uses effect %s outside :allow-effects" molecule.id eff)
           :: acc)
       diagnostics

let validate_tissue file tissue_node tissue diagnostics =
  let diagnostics =
    diagnostics
    |> add_if (tissue.id = "") (diag file (loc_of tissue_node) "genome.tissue_id_missing" "missing tissue id")
    |> add_if (tissue.receptors = [])
         (diag file (loc_of tissue_node) "genome.receptors_missing" "missing tissue :receptors")
    |> add_if (tissue.allow_effects = [])
         (diag file (loc_of tissue_node) "genome.allow_effects_missing" "missing tissue :allow-effects")
    |> add_if (tissue.molecules = [])
         (diag file (loc_of tissue_node) "genome.molecules_missing" "tissue must contain molecules")
  in
  List.fold_left
    (fun acc molecule -> validate_molecule file tissue_node tissue molecule acc)
    diagnostics tissue.molecules

let validate_root file root =
  let genome = genome_of_root file root in
  let diagnostics =
    []
    |> add_if (genome.id = "") (diag file (loc_of root) "genome.id_missing" "missing genome id")
    |> add_if (genome.schema <> "missiond.genome.v1")
         (diag file (loc_of root) "genome.schema_invalid" "genome :schema must be missiond.genome.v1")
    |> add_if
         (not (List.mem genome.activation [ "shadow"; "active"; "rollback" ]))
         (diag file (loc_of root) "genome.activation_invalid" "genome :activation must be shadow, active, or rollback")
    |> add_if (genome.organs = [])
         (diag file (loc_of root) "genome.organs_missing" "genome must contain at least one organ")
  in
  let organ_nodes = child_forms_after_props "organ" root in
  List.fold_left2
    (fun acc organ_node organ ->
      let acc =
        acc
        |> add_if (organ.id = "") (diag file (loc_of organ_node) "genome.organ_id_missing" "missing organ id")
        |> add_if (organ.tissues = [])
             (diag file (loc_of organ_node) "genome.tissues_missing" "organ must contain tissues")
      in
      let tissue_nodes = child_forms_after_props "tissue" organ_node in
      List.fold_left2
        (fun acc tissue_node tissue -> validate_tissue file tissue_node tissue acc)
        acc tissue_nodes organ.tissues)
    diagnostics organ_nodes genome.organs

let validate file =
  try
    let forms = Parser.parse_file file in
    match find_root forms "genome" with
    | None -> [ diag file { line = 1; column = 1 } "genome.missing" "missing genome root" ]
    | Some root -> List.rev (validate_root file root)
  with
  | Reader_error (l, msg) -> [ diag file l "parse.error" msg ]
  | Sys_error msg -> [ diag file { line = 1; column = 1 } "io.error" msg ]
  | Invalid_argument msg -> [ diag file { line = 1; column = 1 } "genome.invalid" msg ]

let validate_dir dir =
  try genome_files dir |> List.map validate |> List.flatten
  with Sys_error msg -> [ diag dir { line = 1; column = 1 } "io.error" msg ]

let string_list_json xs =
  "[" ^ (xs |> List.map json_string |> String.concat ",") ^ "]"

let budgets_json entries =
  Printf.sprintf
    {|{"max_causation_depth":%s,"max_events_per_correlation":%s,"max_cell_runtime_ms":%s,"idempotency_cache_size":%s}|}
    (budget_number ":max-causation-depth" entries "10")
    (budget_number ":max-events-per-correlation" entries "128")
    (budget_number ":max-cell-runtime-ms" entries "300000")
    (budget_number ":idempotency-cache-size" entries "4096")

let molecule_json (m : molecule) =
  Printf.sprintf {|{"id":%s,"on":%s,"when":%s,"effects":%s,"atoms":%s}|}
    (json_string m.id)
    (json_string m.on)
    (match m.when_ with Some value -> json_string value | None -> "null")
    (string_list_json m.effects)
    (string_list_json m.atoms)

let tissue_json (t : tissue) =
  Printf.sprintf
    {|{"id":%s,"receptors":%s,"allow_atoms":%s,"allow_effects":%s,"molecules":[%s],"budgets":%s}|}
    (json_string t.id)
    (string_list_json t.receptors)
    (string_list_json t.allow_atoms)
    (string_list_json t.allow_effects)
    (t.molecules |> List.map molecule_json |> String.concat ",")
    (budgets_json t.budgets)

let organ_json (o : organ) =
  Printf.sprintf {|{"id":%s,"tissues":[%s]}|}
    (json_string o.id)
    (o.tissues |> List.map tissue_json |> String.concat ",")

let genome_json (g : genome) =
  Printf.sprintf {|{"id":%s,"file":%s,"schema":%s,"activation":%s,"organs":[%s]}|}
    (json_string g.id)
    (json_string g.file)
    (json_string g.schema)
    (json_string g.activation)
    (g.organs |> List.map organ_json |> String.concat ",")

let compiled_envelope schema_version source_hash diagnostics payload =
  Printf.sprintf
    {|{"schema_version":%s,"source_hash":%s,"generated_at":null,"diagnostics":[%s],"payload":%s}|}
    (json_string schema_version)
    (json_string source_hash)
    (diagnostics |> List.map diagnostic_to_json |> String.concat ",")
    payload

let parse_genome file =
  let source = read_file file in
  let forms = Parser.parse_source file source in
  match find_root forms "genome" with
  | Some root -> (source, genome_of_root file root)
  | None -> (source, { file; id = ""; schema = ""; activation = ""; organs = [] })

let emit_genomes dir =
  try
    let files = genome_files dir in
    let parsed = files |> List.map parse_genome in
    let sources = parsed |> List.map fst in
    let genomes = parsed |> List.map snd in
    let diagnostics = validate_dir dir in
    let payload =
      Printf.sprintf {|{"genome_dir":%s,"files":%s,"genomes":[%s]}|}
        (json_string dir)
        (string_list_json files)
        (genomes |> List.map genome_json |> String.concat ",")
    in
    let compiled =
      compiled_envelope "missiond.compiled-genomes.v1"
        (source_hash (String.concat "\n" sources))
        diagnostics payload
    in
    print_endline
      (result_json
         ~extra:[ Printf.sprintf {|"compiled":%s|} compiled ]
         (diagnostics = []) diagnostics);
    if diagnostics = [] then 0 else 1
  with Sys_error msg ->
    let d = diag dir { line = 1; column = 1 } "io.error" msg in
    print_endline (result_json false [ d ]);
    1
