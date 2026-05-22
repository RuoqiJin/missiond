open Ast

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

let print_diagnostics diagnostics =
  print_endline (result_json (diagnostics = []) diagnostics);
  if diagnostics = [] then 0 else 1

let usage () =
  prerr_endline
    "Usage: missiond-lispc <emit-json|emit-resolved-v3|emit-v3|emit-runtime-config|emit-semantic-ir|emit-universe|emit-workflows|emit-genomes|check-v3|check-workstation-config|check-workflow|check-workflow-dir|check-genome|check-genome-dir|check-project|check-project-dir|check-auth-domain|check-m6-depth|check-domain-hardening> --file <path>|--dir <path>|--blueprint <path>|--workflow-dir <path>|--genome-dir <path>";
  2

let () =
  let args = Array.to_list Sys.argv |> List.tl in
  match args with
  | "emit-json" :: rest -> (
      match find_arg "--file" rest with
      | Some file -> exit (Emit_json.emit_ast file)
      | None -> exit (usage ()))
  | "emit-v3" :: rest ->
      let file = Option.value ~default:".missiond/v3/missiond-blueprint.lisp" (find_arg "--blueprint" rest) in
      exit (Emit_json.emit_v3 file)
  | "emit-resolved-v3" :: rest ->
      let file =
        Option.value ~default:".missiond/v3/missiond-blueprint.lisp"
          (find_arg "--blueprint" rest)
      in
      exit (Emit_json.emit_resolved_v3 file)
  | "emit-runtime-config" :: rest ->
      let file = Option.value ~default:".missiond/v3/missiond-blueprint.lisp" (find_arg "--blueprint" rest) in
      exit (Emit_json.emit_runtime_config file)
  | "emit-semantic-ir" :: rest ->
      let file = Option.value ~default:".missiond/v3/missiond-blueprint.lisp" (find_arg "--blueprint" rest) in
      exit (Emit_json.emit_semantic_ir file)
  | "emit-universe" :: rest ->
      let file = Option.value ~default:".missiond/v3/missiond-blueprint.lisp" (find_arg "--blueprint" rest) in
      exit (Emit_json.emit_universe file)
  | "emit-workflows" :: rest -> (
      match find_arg "--workflow-dir" rest with
      | Some dir -> exit (Emit_json.emit_workflows dir)
      | None -> exit (usage ()))
  | "emit-genomes" :: rest -> (
      match (find_arg "--genome-dir" rest, find_arg "--dir" rest) with
      | Some dir, _ | None, Some dir -> exit (Genome_schema.emit_genomes dir)
      | None, None -> exit (usage ()))
  | "check-v3" :: rest ->
      let file = Option.value ~default:".missiond/v3/missiond-blueprint.lisp" (find_arg "--blueprint" rest) in
      let expected =
        find_arg "--expected-surfaces" rest |> Option.map split_csv |> Option.value ~default:[]
      in
      exit (print_diagnostics (Schema_v3.validate file expected))
  | "check-workstation-config" :: rest ->
      let file =
        Option.value ~default:".missiond/v3/missiond-blueprint.lisp"
          (find_arg "--blueprint" rest)
      in
      exit (print_diagnostics (Workstation_schema.validate file))
  | "check-workflow" :: rest -> (
      match find_arg "--file" rest with
      | Some file -> exit (print_diagnostics (Workflow_schema.validate file))
      | None -> exit (usage ()))
  | "check-workflow-dir" :: rest -> (
      match (find_arg "--workflow-dir" rest, find_arg "--dir" rest) with
      | Some dir, _ | None, Some dir ->
          exit (print_diagnostics (Workflow_schema.validate_dir dir))
      | None, None -> exit (usage ()))
  | "check-genome" :: rest -> (
      match find_arg "--file" rest with
      | Some file -> exit (print_diagnostics (Genome_schema.validate file))
      | None -> exit (usage ()))
  | "check-genome-dir" :: rest -> (
      match (find_arg "--genome-dir" rest, find_arg "--dir" rest) with
      | Some dir, _ | None, Some dir ->
          exit (print_diagnostics (Genome_schema.validate_dir dir))
      | None, None -> exit (usage ()))
  | "check-project" :: rest -> (
      match find_arg "--file" rest with
      | Some file -> exit (print_diagnostics (Project_schema.validate file))
      | None -> (
          match find_arg "--blueprint" rest with
          | Some file -> exit (print_diagnostics (Project_schema.validate file))
          | None -> exit (usage ())))
  | "check-project-dir" :: rest -> (
      match find_arg "--dir" rest with
      | Some dir -> exit (print_diagnostics (Project_schema.validate_project_dir dir))
      | None -> exit (usage ()))
  | "check-auth-domain" :: rest -> (
      match (find_arg "--file" rest, find_arg "--dir" rest) with
      | Some file, _ -> exit (print_diagnostics (Project_schema.validate_auth_domain file))
      | None, Some dir -> exit (print_diagnostics (Project_schema.validate_auth_domain_dir dir))
      | None, None -> exit (usage ()))
  | "check-m6-depth" :: rest -> (
      match find_arg "--dir" rest with
      | Some dir -> exit (print_diagnostics (Project_schema.validate_m6_depth_dir dir))
      | None -> exit (usage ()))
  | "check-domain-hardening" :: rest -> (
      match find_arg "--dir" rest with
      | Some dir -> exit (print_diagnostics (Project_schema.validate_domain_hardening_dir dir))
      | None -> exit (usage ()))
  | _ when has_flag "--help" args -> exit (usage ())
  | _ -> exit (usage ())
