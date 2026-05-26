(intent :id "20260526-dispatch-hint-normalization"
  :summary "Normalize workstation dispatch hint spelling so claude_code and claude-code do not create false reroute noise."
  :scope ["autopilot dispatch hint matching" "V3 workstation runtime policy" "checker pin"]
  :acceptance ["dispatch hint matching treats underscore and hyphen spellings as equivalent" "focused autopilot test passes" "workstation pool checker passes"]
  :created_at "2026-05-26")
