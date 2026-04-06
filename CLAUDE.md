# MissionD — Semantic Role Architecture Fix

Fixing the semantic_role classification pipeline:
1. `message_handler.rs:198` — overly broad user→system remap rule
2. Clear polluted `message_labels` + `conversation_turns`
3. Architectural hardening of the COALESCE override chain
