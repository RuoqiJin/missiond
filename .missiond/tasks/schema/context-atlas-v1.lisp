;; MissionD context-atlas v1
;; Purpose: a Lisp-shaped, machine-checkable navigation map that future
;; waves attach to a task contract via :context-atlas-path so workers
;; receive precise file anchors, grep keywords, task-focus first-reads,
;; avoid notes, and a canonical read-order BEFORE running broad repository
;; search. wave29-01 ships schema + read-only checker
;; scripts/check-context-atlas.mjs; wave29-03 prep CLI and wave29-06
;; ready-queue planner consume the named exports.
;;
;; Loadbearing rule: a context-atlas is NAVIGATION METADATA ONLY. It is
;; NOT a behavioral contract. The atlas does not authorise any backend
;; switch, does not control dispatch, and does not promote any
;; recommendation. Its only job is to reduce navigation misses. The
;; checker validates structure; the wave29 prep / planner tools read
;; projected atlases to surface anchors in briefs and ready-queue output.
;;
;; Schema strings accepted:
;;   missiond.context-atlas.v1           — durable per-task or per-wave atlas
;;   missiond.context-atlas.dispatch.v0  — transitional dispatch-time atlas
;;                                         (the wave29 dispatch atlas declares
;;                                         this string; the schema accepts it
;;                                         so the wave29 file validates as-is
;;                                         and future waves can migrate to v1
;;                                         without churning the dispatch
;;                                         layer mid-flight)
;; Drift outside this set is a structural error.

(context-atlas-schema missiond.context-atlas.v1
  :version "v1"
  :status "code-aligned — schema + read-only checker scripts/check-context-atlas.mjs (wave29-01); intentionally NOT wired to runtime dispatch and NOT a behavioral contract. wave29-03 prep CLI surfaces atlas anchors in thin briefs; wave29-06 ready-queue planner reads atlas first-reads to order workers."
  :checker "scripts/check-context-atlas.mjs"
  :seed nil

  (purpose
    "Make navigation metadata a structured Lisp record instead of free-form prose embedded in task briefs."
    "Pin the cross-wave invariant that an atlas is NAVIGATION ONLY — it does not authorise any backend switch and does not control dispatch."
    "Encode file anchors with optional :purpose / :grep so workers can rg into a hot-path Rust module instead of reading the whole file."
    "Encode task_focus entries that name first-reads + avoid notes so the dispatch-time renderer can pin the right anchors per task."
    "Encode top-level grep / avoid entries so a wave-level atlas can publish cross-task anchors / no-go zones without inventing a new entry head per wave.")

  (file-shape
    :file ".missiond/tasks/<wave>/context-atlas.lisp OR .missiond/atlas/<id>.lisp"
    :form (context-atlas <id>
            :schema "missiond.context-atlas.v1"
            ;; required navigation header
            :read-order [<repo-relative-path-string> ...]
            ;; one of :purpose | :goal is required (synonyms for the same field)
            :purpose <string>
            ;; optional header fields
            :goal <string>
            :wave <wave-id>
            :version <string>
            :description <string>
            :generated_at <iso-8601-string>
            :generator <string>
            ;; entry forms (zero or more of each; at least one entry overall)
            <global-anchors-or-file-or-task-focus-or-grep-or-avoid> ...))

  (header-fields
    [:schema :read-order :purpose :goal :wave :version :description :generated_at :generator])

  ;; Top-level entry heads.
  ;;   global-anchors  — container; children MUST be (file ...) entries
  ;;   task-focus      — container; children MUST be (task ...) entries
  ;;   file            — flat per-file anchor entry (top-level shorthand)
  ;;   task_focus      — flat per-task entry (top-level shorthand for one task)
  ;;   grep            — top-level cross-task grep anchor
  ;;   avoid           — top-level avoid note (no-go zone or noise warning)
  (entry-heads
    [global-anchors task-focus file task_focus grep avoid])

  (accepted-schema-strings
    ["missiond.context-atlas.v1"
     "missiond.context-atlas.dispatch.v0"])

  (header-contract
    (:schema "literal string in (accepted-schema-strings ...); missing or mismatched schema is a structural error.")
    (:read-order "vector of repo-relative path strings; non-empty; absolute / ~ / .. paths are rejected; entries describe canonical reading sequence for workers.")
    (:purpose "non-empty string describing what the atlas helps with. EITHER :purpose OR :goal MUST be present (synonyms for the same field).")
    (:goal "non-empty string; synonym for :purpose; same constraint applies.")
    (:wave "OPTIONAL non-empty kebab id matching ^[a-z][a-z0-9-]*$; SHOULD match the directory name under .missiond/tasks/<wave>/ when the atlas is wave-scoped.")
    (:version "OPTIONAL non-empty version string (free-form).")
    (:description "OPTIONAL free-form prose for humans; never load-bearing for validation.")
    (:generated_at "OPTIONAL ISO-8601 timestamp string identifying when the atlas was emitted.")
    (:generator "OPTIONAL string identifying which CLI / handler emitted the atlas."))

  (file-entry-contract
    "Form: (file <repo-relative-path-string> :purpose <string>? :grep [<string> ...]?)"
    (:path "second form of a (file ...) entry; non-empty repo-relative path string; absolute / ~ / .. paths are rejected.")
    (:purpose "OPTIONAL non-empty string describing why the worker should read this file.")
    (:grep "OPTIONAL vector of non-empty grep anchor strings (substring or regex); empty strings are rejected; an empty vector is allowed."))

  (task-entry-contract
    "Form: (task <task-id> :first-reads [<string> ...]? :avoid [<string> ...]?)
     Form: (task_focus <task-id> :first-reads [<string> ...]? :avoid [<string> ...]?)
     Both heads are accepted; (task_focus ...) is the top-level shorthand for one task entry."
    (:task_id "second form of a (task ...) / (task_focus ...) entry; non-empty kebab id matching ^[a-z0-9][a-z0-9._-]*$; unique within a single atlas.")
    (:first-reads "OPTIONAL vector of repo-relative path strings the worker should read first; entries follow the same path-style rules as :read-order.")
    (:avoid "OPTIONAL vector of non-empty free-form strings warning what NOT to read or scan."))

  (grep-entry-contract
    "Form: (grep <pattern-string> :purpose <string>?)
     Top-level grep anchor; pattern MUST be a non-empty string (substring or regex)."
    (:pattern "second form of a (grep ...) entry; non-empty string; empty pattern is a structural error.")
    (:purpose "OPTIONAL non-empty string describing why this grep anchor matters."))

  (avoid-entry-contract
    "Form: (avoid <free-form-string> ...)
     Top-level avoid note; one or more non-empty free-form strings explaining what NOT to scan or edit."
    (:strings "all child forms after the head MUST be non-empty strings (substrings of larger prose are fine)."))

  (container-contract
    "Container forms (global-anchors / task-focus) carry no keyword props of their own; all children MUST be the matching leaf entry."
    (:global-anchors-children "every list child of (global-anchors ...) MUST have head `file`.")
    (:task-focus-children     "every list child of top-level (task-focus ...) MUST have head `task`."))

  (uniqueness
    "Within a single (context-atlas ...) form, file paths MUST be unique across ALL (file ...) leaves (whether nested under (global-anchors ...) or declared top-level)."
    "Within a single (context-atlas ...) form, task ids MUST be unique across ALL (task ...) and (task_focus ...) entries."
    "Within a single file the schema rejects MULTIPLE top-level (context-atlas ...) forms — atlases are single-document.")

  (validation-contract
    :file-must-have-header [:schema :read-order]
    :file-must-have-one-of [:purpose :goal]
    :unique-per-atlas [:file_path :task_id]
    :enum-checked [:schema]
    :path-fields [:read-order :file-path :first-reads]
    :rejects
      ["schema mismatch (:schema not in accepted-schema-strings)"
       "missing required header field (:schema or :read-order)"
       "missing both :purpose and :goal"
       "unknown header field"
       ":read-order missing or empty"
       ":read-order entry that is absolute / ~ / contains \"..\" traversal"
       "(file ...) entry with absolute / ~ / .. path"
       "(file ...) entry with empty :purpose string"
       "(file ...) entry with an empty :grep anchor string"
       "duplicate file path inside a single atlas"
       "(task ...) / (task_focus ...) entry with malformed :task_id"
       "duplicate task id inside a single atlas"
       "(task ...) / (task_focus ...) :first-reads entry that is absolute / ~ / contains \"..\""
       "(task ...) / (task_focus ...) :avoid entry that is empty"
       "(grep ...) entry with empty pattern"
       "(grep ...) entry with empty :purpose when present"
       "(avoid ...) entry with no children or with an empty/non-string child"
       "(global-anchors ...) container with a non-`file` child"
       "(task-focus ...) container with a non-`task` child"
       "multiple (context-atlas ...) forms in a single file"
       "unknown top-level entry head"
       "atlas with zero entries"]
    :no-prose
      "atlas entries are S-expressions; narrative belongs in :purpose / :avoid strings only.")

  (checker-contract
    :input "stdin (--stdin) OR ad hoc atlas.lisp files"
    :modes [single-file stdin dry-fixture]
    :flags [--json --stdin --dry-fixture]
    :json-shape "{ ok, files, errors[], warnings[], atlases_validated }"
    :rejects
      ["everything in :validation-contract :rejects above"
       "any input that is not a single (context-atlas ...) form"]
    :non-goal
      "checker does NOT call git, does NOT shell out, does NOT touch the network or any LLM, and does NOT verify that the referenced file paths exist on disk. wave29-03 prep CLI and wave29-06 planner are responsible for joining atlas to live files; wave29-01 validates structure only.")

  (cross-wave-invariant
    "Context atlases are NAVIGATION METADATA ONLY. No wave reads these to alter live dispatch."
    "An atlas does NOT authorise any backend switch and does NOT promote any router recommendation."
    "File path uniqueness inside an atlas keeps prep / planner output deterministic; duplicate paths break byte-stable output and are a structural error.")

  (non-goals
    "The schema does not validate that referenced files exist on disk — the prep CLI / planner does that join."
    "The schema does not score backends or pick a winner — backend selection stays in router-recommendation surfaces."
    "The schema does not encode rendering policy — the brief renderer decides how to surface anchors."
    "The schema does not own the dispatch-time wave29 atlas's exact field names; instead it accepts both :purpose and :goal as synonyms so v1 adoption does not require rewriting wave29 dispatch artifacts."
    "The schema is not a behavioral contract — workers MAY ignore atlas anchors when the task already names the right files; the atlas only reduces navigation misses, it never blocks work."))
