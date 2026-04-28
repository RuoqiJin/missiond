;; Wave 33 dispatch plan.

(dispatch-plan wave33-lisp-isomorphism-cleanup
  :schema "missiond.dispatch-plan.v1"
  :wave wave33
  :goal "Continue the code-to-Lisp isomorphism cleanup now that MissionD can run ClaudeCode tasks through dynamic Opus slots."
  :ready-queue true
  :max-parallel 1
  :measurement-goal "Confirm that the post-wave32 Autopilot runtime can dispatch another Opus coding task without prompt/tool-contract drift or duplicate re-dispatch."
  :notes
    ["This wave intentionally does not add a new product feature."
     "It moves observed prompt/tool behavior into the V3 Lisp blueprint, then aligns Autopilot projection code to that contract."
     "The screenshot issue to fix: repeated objective text plus unconditional board MCP self-close instructions when those tools may not be attached to the ClaudeCode session."])
