open Ast

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

let parse_source file source =
  let p = make file source in
  parse_forms p None

let parse_file file = parse_source file (read_file file)
