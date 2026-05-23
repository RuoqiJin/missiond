// Compatibility reader for V3 checkers that still validate source-shaped Lisp
// details after missiond-lispc has resolved the compiler-active shard graph.
//
// New V3 semantic authority must come from compiled contract/semantic facts.
// This module exists only to keep legacy source-shape assertions centralized
// while preventing check-v3* scripts from importing the raw parser directly.
export {
  head,
  isList,
  keywordPropBool,
  keywordPropText,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
} from './missiond_lisp.mjs';
