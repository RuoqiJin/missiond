;; ════════════════════════════════════════════════════════════════
;; MissionD Pure Utility Declarations
;; Pattern: pure-utility (algorithm isolation chamber)
;; Status: Phase 1 — semantic parser extract
;; ════════════════════════════════════════════════════════════════

;; --- Semantic Parser Helpers ---
;; These are pure functions extracted from missiond-core/src/semantic/.
;; No &self, no side effects, no I/O.

(pure-utility semantic-parsing
  :target "missiond-core/src/semantic/gen_parsing.rs"

  (fn is_spinner_char
    :params ((c char))
    :returns bool
    :doc "Check if a character is a known spinner glyph (braille dots, bars, etc)"
    :test-cases
      ((case "braille-dot" :input "'⠋'" :expected true)
       (case "braille-bar" :input "'⣾'" :expected true)
       (case "normal-a" :input "'A'" :expected false)
       (case "space" :input "' '" :expected false)))

  (fn split_args
    :params ((input "&str"))
    :returns "Vec<String>"
    :doc "Split comma-separated arguments respecting quoted strings"
    :test-cases
      ((case "simple" :input "\"a, b, c\"" :expected "vec![\"a\".into(), \"b\".into(), \"c\".into()]")
       (case "quoted" :input "\"name: \\\"foo\\\", path: /src\"" :expected "vec![\"name: \\\"foo\\\"\".into(), \"path: /src\".into()]")))

  (fn extract_phase_from_parens
    :params ((parens_content "&str"))
    :returns "Option<String>"
    :doc "Parse phase hint (thinking/tool) from status bar parenthesized content like '3m 2s · thinking'"
    :test-cases
      ((case "thinking" :input "\"3m 2s · thinking\"" :expected "Some(\"thinking\".into())")
       (case "tool-running" :input "\"1s · Read\"" :expected "Some(\"Read\".into())")
       (case "just-timer" :input "\"5s\"" :expected None)))

  (fn sanitize_line
    :params ((line "&str"))
    :returns String
    :doc "Strip box-drawing characters and trim line for text analysis"
    :test-cases
      ((case "clean" :input "\"hello world\"" :expected "\"hello world\".into()")
       (case "box-chars" :input "\"│ hello │\"" :expected "\"hello\".into()")))

  (fn has_activity_timer
    :params ((line "&str"))
    :returns bool
    :doc "Detect (Ns · or (Nm Ns · timer pattern indicating active processing"
    :test-cases
      ((case "seconds" :input "\"(5s · thinking)\"" :expected true)
       (case "min-sec" :input "\"(3m 2s · Read)\"" :expected true)
       (case "no-timer" :input "\"hello world\"" :expected false)))

  (fn is_idle_prompt
    :params ((lines "&[&str]"))
    :returns bool
    :doc "Check if terminal screen shows an idle prompt (❯ or > at bottom)"
    :test-cases
      ((case "chevron" :input "&[\"❯\"]" :expected true)
       (case "gt" :input "&[\"> \"]" :expected true)
       (case "output" :input "&[\"Hello world\"]" :expected false))))

;; --- String Safety Helpers ---
;; Global safe truncation primitives. Replaces all dangerous &s[..N] byte slicing.

(pure-utility string-helpers
  :target "missiond-core/src/util/gen_string_helpers.rs"

  (fn safe_byte_truncate
    :params ((s "&str") (max_bytes usize))
    :returns "&str"
    :doc "Truncate string to at most max_bytes, never breaking a char boundary. Returns a valid UTF-8 slice."
    :test-cases
      ((case "ascii-within" :input "\"hello\", 10" :expected "\"hello\"")
       (case "ascii-exact" :input "\"hello\", 5" :expected "\"hello\"")
       (case "ascii-cut" :input "\"hello world\", 5" :expected "\"hello\"")
       (case "cjk-boundary" :input "\"abc你好\", 4" :expected "\"abc\"")
       (case "cjk-boundary-6" :input "\"abc你好\", 6" :expected "\"abc你\"")
       (case "box-drawing" :input "\"ab═cd\", 4" :expected "\"ab\"")
       (case "empty" :input "\"\", 100" :expected "\"\"")
       (case "zero-max" :input "\"hello\", 0" :expected "\"\"")))

  (fn safe_char_truncate
    :params ((s "&str") (max_chars usize))
    :returns "&str"
    :doc "Truncate string to at most max_chars Unicode characters."
    :test-cases
      ((case "within" :input "\"hello\", 10" :expected "\"hello\"")
       (case "cut" :input "\"hello world\", 5" :expected "\"hello\"")
       (case "cjk" :input "\"你好世界\", 2" :expected "\"你好\""))))

;; --- Token & Budget Helpers ---
;; Pure math functions for context window management.

(pure-utility token-budget
  :target "missiond-daemon/src/context/gen_budget.rs"

  (fn estimate_tokens
    :params ((text "&str"))
    :returns usize
    :doc "Rough token count estimate: chars / 4 for English, / 2 for CJK"
    :test-cases
      ((case "english" :input "\"hello world\"" :expected 3)
       (case "empty" :input "\"\"" :expected 0)))

  (fn allocate_budget
    :params ((total usize) (source_count usize))
    :returns "Vec<usize>"
    :doc "Divide token budget across N sources with diminishing returns per source"
    :test-cases
      ((case "single" :input "1000, 1" :expected "vec![1000]")
       (case "two" :input "1000, 2" :expected "vec![600, 400]"))))
