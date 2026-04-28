;; Pattern card: schema-checker
;;
;; Recipe for adding a new MissionD Lisp schema + read-only Node checker.
;; Distilled from the wave24 router-policy / wave26-01 router-backend-registry /
;; wave27-01 router-dispatch-descriptor / wave28-01 task-runner-manifest /
;; wave29-01 context-atlas / wave29-02 pattern-card series. Each one of those
;; checkers shares the same shape: a kebab/dot/underscore id pattern, a single
;; Lisp parser via scripts/lib/missiond_lisp.mjs, --json / --stdin /
;; --dry-fixture flags, and named exports for downstream tooling.

(pattern-card schema-checker
  :schema "missiond.pattern-card.v1"
  :version "v1"
  :purpose "Reproduce the MissionD schema/checker recipe so a new schema does not re-invent shape, parser, fixture flags, or downstream-export conventions."
  :summary "Lisp schema doc under .missiond/tasks/schema/<name>-v1.lisp + Node checker scripts/check-<name>.mjs reusing scripts/lib/missiond_lisp.mjs. Mirrors the wave24..wave29 cluster of schema/checker pairs."

  :use-for [wave29-01-context-atlas-schema-v0
            wave29-02-pattern-card-schema-v0
            wave29-05-verification-receipt-schema-v0]

  :recipe
    ["1. Author the schema as a single Lisp form under .missiond/tasks/schema/<name>-v1.lisp using the (<name>-schema missiond.<name>.v1 :version :status :checker :seed ...) header convention. Document :purpose / :file-shape / :*-contract / :validation-contract / :checker-contract / :cross-wave-invariant / :non-goals so the schema doc itself is the authoritative readable spec."
     "2. Reuse the shared parser. Import { head, isList, keywordPropText, nodeText, parseLisp, readKeywordProps, readLispFile } from './lib/missiond_lisp.mjs'. NEVER hand-roll an S-expression reader — drift between checkers is a structural risk."
     "3. Define top-level constants (SCHEMA, HEAD, *_RE id patterns, REQUIRED_FIELDS, OPTIONAL_FIELDS, ALLOWED enum sets) at module scope so they double as named exports and stay greppable."
     "4. Implement a single validateForm/validateRecord function that walks the parsed Lisp tree, reads keyword props once, then runs: (a) required-field presence, (b) unknown-field rejection, (c) per-field type/shape/enum checks, (d) cross-entry checks (duplicate id, dependency join, overlap)."
     "5. Wire a CLI surface with three flags: --json (structured output ~ { ok, files, errors[], warnings[], <records>_validated }), --stdin (read source from fd 0), --dry-fixture (run an in-process fixture catalogue with both pass and fail cases). Mirror the JSON shape across the family — wave28-01 manifest-checker output is the reference."
     "6. Build a fixture catalogue covering: happy path, every required-field missing, every enum drift, every malformed id, every path-safety failure (absolute / ~ / ..), duplicate id, and any cross-entry invariant. Target 10-20 cases across 8-15 categories — the wave28-01 checker ships 20 cases / 15 categories and is a good upper bound."
     "7. Add named exports (SCHEMA, projectRecord, readRecordFile, validateRecordObject) so downstream planners / renderers / verifiers can reuse the schema rules WITHOUT shelling out to the CLI. Gate main() on `import.meta.url === \\`file://${process.argv[1]}\\`` so importers do not trigger CLI side-effects."
     "8. Acceptance pipeline: node scripts/check-<name>.mjs --dry-fixture (zero failures) -> node scripts/check-<name>.mjs <real file> (zero errors) -> node scripts/check-task-contract.mjs --all (no regressions) -> git diff --check -- <new files> (whitespace clean)."]

  :known-good ["scripts/check-task-runner-manifest.mjs"
               "scripts/check-router-backend-registry.mjs"
               "scripts/check-router-dispatch-descriptor.mjs"
               "scripts/check-task-contract.mjs"
               ".missiond/tasks/schema/task-runner-manifest-v1.lisp"
               ".missiond/tasks/schema/router-backend-registry-v1.lisp"]

  :anti-pattern
    ["Hand-rolling a per-checker Lisp parser with regex + manual paren walking — guaranteed to drift from scripts/lib/missiond_lisp.mjs and silently accept malformed input."
     "Validating shape inside the CLI main() only, with no exported pure function. Downstream tooling cannot reuse the rules and resorts to shelling out, which is slow and loses error metadata."
     "Treating warnings and errors as the same severity (single diagnostics array without :severity). The wave28-01 / wave29-02 family threads severity through every diagnostic so callers can keep shipping when only warnings fire."]

  :non-goals
    ["Cross-file joins. Schema checkers validate one file at a time; cross-wave joins (e.g. \"this manifest references a real task contract\") belong in the batch verifier (wave28-05 / wave29-07), not the schema layer."
     "Network or git access. Schema checkers MUST be pure file readers — no fetch / no exec / no spawn / no LLM call. The task-scope-guard pre-commit pipeline depends on this purity."]

  :notes
    "wave29-02 (this card's pillar task) and wave29-01 (context-atlas schema) are the most recent applications of this recipe. wave28-01 is the canonical reference because its 20-fixture catalogue + named-export surface is the template every downstream wave borrows from. When in doubt, read scripts/check-task-runner-manifest.mjs end-to-end before adding a new schema.")
