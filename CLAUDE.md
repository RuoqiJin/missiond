# MECHANIC SANDBOX — CARTOGRAPHY MODE

You are a top-tier systems architect operating inside an isolated sandbox.

## ABSOLUTE RULE: READ-ONLY FOR ALL NON-LISP FILES
1. You MUST NOT modify, create, or delete any `.rs`, `.toml`, `.yaml`, `.json`, or other non-Lisp files.
2. You may ONLY read Rust source files to analyze patterns.
3. You may ONLY edit `.lisp` files in the `.missiond/` directory.

## Your Mission
1. Deep-read the Rust source files listed below to understand the architecture.
2. Identify communication patterns, concurrency models, state machines, or API structures.
3. Open `.missiond/intent.lisp` and find the [GAP] placeholder for this component.
4. Replace the [GAP] with a well-designed `(pattern ...)` or `(component ...)` S-expression that captures the architecture you discovered.
5. Ensure the Lisp syntax is valid and properly nested.
6. Commit with: `git add -A && git commit -m "[MECHANIC] cartography: <component-name>"`

## Source Files to Analyze
`missiond-core/src/semantic/types.rs`, `missiond-core/src/semantic/state.rs`, `missiond-core/src/semantic/confirm.rs`, `missiond-core/src/semantic/tool.rs`, `missiond-core/src/semantic/fingerprint.rs`, `missiond-core/src/semantic/patterns.rs`, `missiond-core/src/semantic/gemini_state.rs`

## Context
**Component:** semantic-parser
**Intent declaration:**
```lisp
(component semantic-parser :pattern-gap "semantic-parser" :certainty 0 :target
  "missiond-core/src/semantic/" :files
  (types.rs state.rs confirm.rs tool.rs fingerprint.rs patterns.rs
    gemini_state.rs) :description
  "Terminal output pattern matching and state inference.
        Parses raw PTY screen lines into structured states using regex
        fingerprints. Not a state-machine (it FEEDS state machines).
        It is a parser/recognizer pattern." :sub-patterns
  ((screen-parser "\"line-by-line terminal output analysis\"")
    (fingerprint-db "\"regex pattern database for state detection\"")
    (confirm-parser "\"permission dialog structure extraction\"")
    (tool-recognizer "\"tool invocation output parsing\"")
    (multi-engine "\"per-engine parser variants (Claude/Gemini)\"")))
```
