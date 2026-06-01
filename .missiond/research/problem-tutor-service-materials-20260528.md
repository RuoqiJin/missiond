# Problem Tutor Service Materials

Date: 2026-05-28
Working service id: `problem-tutor`
Purpose: image-first problem solving tutor with search-grounded original problem recovery, multi-model solving/explanation, generated visualization, and follow-up Q&A.

## Source Order

Read order followed:

1. MissionD V3 SSOT: `.missiond/v3/missiond-blueprint.lisp`
2. Shard index: `.missiond/v3/shards/index.lisp`
3. Active shards: universe project registry, service runtime, infrastructure, data residency, control plane, memory/conversation, workstation/router policy surfaces
4. Compiled runtime ABI: canonical deployed projection under `/Users/jinchen/.missiond/runtime/missiond/compiled/*.json`; repo-local `.missiond/v3/runtime/compiled` is not present and should be treated as cold/dev compatibility only
5. Project SSOT and implementation evidence for wepub, router, search-center, and asr

## Relevant MissionD Facts

- MissionD V3 is file-first Lisp authority. Runtime hot paths should consume compiled runtime projections, not raw Lisp.
- Production compiled projections currently live under `/Users/jinchen/.missiond/runtime/missiond/compiled`.
- Project universe confirms:
  - `wechat-publisher` / wepub: M6, independent Rust + Next app at `/Users/jinchen/Projects/wechat-publisher`
  - `router`: M6, Rust service at `/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/router`
  - `search-center`: M5, Rust + Next service, gaps are deep-research e2e, browser OAuth, final M6 report
  - `asr`: M6, Rust + Next service with upload, object storage, SSE/session patterns

## Wepub Reference Stack

Use wepub as the service template:

- Backend: Rust 2021, Axum 0.8, Tokio, SQLx/Postgres, reqwest, tower/tower-http, tracing, dotenvy.
- Frontend: Next 16, React 19, Tailwind 4, TypeScript 5, lucide-react.
- Auth: XJP Auth Bearer token; author-only write guard.
- Data: xjp-pg-prod Postgres.
- Deployment: Deploy Center -> GCP runtime container, Caddy proxy, Cloudflare DNS; frontend through Vercel.
- SSOT shape: `.missiond/intent.lisp`, backend blueprint, frontend blueprint, operations blueprint, final M6 report, `.missiond/check.sh`.
- Regression shape: backend cargo test/check, frontend build, renderer/API smoke, auth guard smoke.

Important difference: wepub's current AI endpoint calls OpenRouter directly. The new tutor service should not copy that path. It should call XJP Router for model access, and Search Center / router search for aggregate search.

## Router And Search Facts

Router:

- Public compatible chat path: `POST /v1/chat/completions`.
- OpenAI-style multimodal messages are supported. `content` may be an array containing `text`, `image_url`, `video_url`, `audio_url`, or `file_url`.
- `gpt-5.5` exists in router `config/xjp.toml` as a Meow61-backed logical model.
- Gap: `gpt-5.5` is not present in `src/inbound/models.rs` model listing, and its billing id is still a TODO mapped to Claude Sonnet pricing. Before production, add the public model list entry and pricing SKU.
- `claude-opus-4.6`, `claude-opus-4.6-thinking`, and `claude-opus-4.6-claudecode` exist in router config.
- `gemini-3.1-pro-preview` exists in router config; compiled MissionD runtime also uses `gemini-3.1-pro`.
- Gap: `gemini-3.5-flash` was not found in router config or model listing. Existing nearby models are `gemini-3-flash-preview` and `gemini-3.1-flash-lite`. If the product requirement is specifically Gemini 3.5 Flash, add the router alias, connector route, model list entry, and pricing before relying on it.

Search:

- Router search SSOT exposes `POST /v1/search` and `POST /v1/workflows/search`.
- Search Center wraps router search, stores history/cache, and exposes `POST /v1/search` plus `POST /v1/research`.
- Search Center Deep Research already has a pattern for planner -> search fanout -> evidence curation -> synthesis, with fail-fast behavior when structured output is missing.
- For this tutor service, use Search Center for "find original problem" only when OCR confidence is low or the uploaded image is unclear. Do not replace the uploaded problem unless strict verification passes.

## Main Flow

1. Upload and session creation
   - Browser uploads one or more problem images.
   - Backend authenticates with XJP Auth, stores originals in object storage, creates `tutor_sessions` and `tutor_assets`.
   - Return `session_id`; stream progress over SSE or poll status.

2. OCR and problem reconstruction
   - Call router `gpt-5.5` with image content.
   - Output strict JSON: extracted text, math notation, diagram/table/figure description, visible labels, confidence, unclear regions, and search queries.
   - If the image has math diagrams, record both textual statement and structured visual description.

3. Original-problem recovery
   - Trigger only when text/image is unclear, OCR confidence is low, or key fields are missing.
   - Query Search Center quick search with the extracted keywords/formulas.
   - Fetch candidate snippets/raw content where available.
   - Ask `gpt-5.5` to compare each candidate against the uploaded image extraction.
   - Strict rule: replace with online original only when wording, numbers, options, diagram constraints, and answer target are all equivalent. If any mismatch remains, use the uploaded image extraction.
   - Store candidates and verification decision for audit.

4. Solve
   - Call router `gpt-5.5`.
   - Input: chosen problem text, image/diagram reconstruction, original-image refs, and candidate verification notes.
   - Output strict JSON: final answer, step-by-step solution, assumptions, common mistakes, and confidence.

5. Visualization plan
   - Call `gpt-5.5`.
   - Input: problem + solution.
   - Output text-only visual spec: learning objective, scene breakdown, variables to animate/highlight, required geometry, constraints, and target artifact type.

6. Visualization implementation
   - Call Gemini Flash target through router.
   - Product target is `gemini-3.5-flash`, but router currently needs an alias/config update.
   - Output one controlled artifact:
     - `svg` for static/vector math visuals
     - `html_fragment` for small interactive/animated visuals
     - `lottie_json` only if the frontend renderer supports it
   - Enforce: no external network, no external scripts, bounded width/height, max byte size, sandboxed iframe for HTML, CSP, and sanitizer before persistence.

7. Explanation polish and presentation
   - Call `gemini-3.1-pro-preview` or the configured Gemini 3.1 Pro alias.
   - Input: original GPT solution, visual spec/artifact, and target student level.
   - Output polished explanation, while preserving math correctness and not changing the final answer.
   - Present three panes/sections: GPT solution, polished explanation, visualization.

## Follow-up Q&A Flow

Frontend interaction:

- User can drag-select text from any answer section and quote it into the question composer.
- Payload includes selected text, source section, character range when available, `session_id`, and free-form user question.
- Reserve ASR input by keeping the composer API text-first but accepting future `asr_transcript_id` / `audio_asset_id`.

Model chain:

1. `gpt-5.5` intent recognition
   - Classify: confusion about a step, asks for proof, asks for alternate method, challenges answer, asks about visualization, asks a new related problem, or general chat.
   - Include selected quote context.

2. `claude-opus-4.6` analysis
   - Diagnose where the student's question or misconception is.
   - Decide whether search is needed. Search should be used only for external facts, source/original-problem disputes, or time-sensitive/contextual claims.

3. `gpt-5.5` answer
   - Use all session context: uploaded image extraction, verified original decision, solution, visualization plan/artifact, previous Q&A, selected quote, Opus diagnosis, and optional search evidence.
   - Answer the actual student question, not the whole problem again unless necessary.

4. `gemini-3.1-pro-preview` polish
   - Improve clarity and tutoring tone.
   - Preserve formulas, final answer, and citations/evidence if search was used.

## Suggested API Surface

Backend:

- `POST /api/problems` multipart image upload, creates session.
- `GET /api/problems/{session_id}` returns current state/result.
- `GET /api/problems/{session_id}/events` SSE progress.
- `POST /api/problems/{session_id}/questions` follow-up text question.
- `POST /api/problems/{session_id}/asr-placeholder` reserved no-op or feature-gated endpoint for later speech input.
- `GET /api/health`.

Frontend:

- Login-gated workspace as first screen.
- Upload/camera paste zone.
- Progress timeline for OCR, search verification, solving, visualization, explanation.
- Result view with selectable text and quote composer.
- Visualization renderer using SVG inline sanitizer or HTML sandbox iframe.

## Suggested Data Model

- `tutor_sessions`: id, user_id, tenant_id, status, subject_hint, created_at, completed_at, error.
- `tutor_assets`: id, session_id, kind, object_url/key, mime, sha256, width, height, source.
- `tutor_problem_extracts`: session_id, model, extracted_json, confidence, unclear_regions, chosen_source.
- `tutor_original_candidates`: id, session_id, query, url, title, snippet, candidate_text, verification_json, accepted.
- `tutor_solution_steps`: session_id, model, solution_json, visual_plan_json, explanation_json.
- `tutor_visual_artifacts`: session_id, type, content, sanitizer_status, dimensions, version.
- `tutor_followup_turns`: id, session_id, selected_quote_json, intent_json, diagnosis_json, answer_json, polished_answer, created_at.
- `tutor_usage_events`: session_id, provider/model, phase, token/cost metadata, router request_id.

## Policy And Safety Boundaries

- Fail fast on missing router/search credentials; do not fabricate AI/search output.
- Do not silently replace the uploaded problem with a web result. Strict verification is mandatory.
- Preserve original uploaded image evidence and extraction JSON.
- Treat student uploads as private content; use object storage with scoped access and retention policy.
- Generated HTML must be sandboxed; SVG/HTML must be sanitized before display.
- Store model phase outputs separately so follow-up answers can cite the exact source step.
- Model names should be service config values, not hardcoded in handlers.

## Recommended New Project Shape

Mirror wepub:

- Root: `/Users/jinchen/Projects/problem-tutor` or agreed service name.
- Kind: `rust-nextjs-service`.
- Backend: Rust/Axum + SQLx/Postgres + reqwest + tower-http + tracing.
- Frontend: Next 16 + React 19 + Tailwind 4 + TypeScript + lucide-react.
- Auth: XJP Auth browser PKCE and backend Bearer validation.
- Storage: reuse ASR-style R2 primary with OSS fallback if needed.
- Runtime: Deploy Center GCP backend container, Caddy route, Vercel frontend, Cloudflare DNS.
- SSOT files to create:
  - `.missiond/intent.lisp`
  - `.missiond/backend/problem-tutor-backend-blueprint.lisp`
  - `.missiond/frontend/problem-tutor-frontend-blueprint.lisp`
  - `.missiond/operations/problem-tutor-operations-blueprint.lisp`
  - `.missiond/check.sh`
  - later `.missiond/evidence/problem-tutor-final-m6-report.lisp`

## Next-Step Gaps

1. Choose service id, repo root, public domains, and whether backend routes live under `auth.xiaojinpro.com/<prefix>` or a dedicated API domain.
2. Add/verify router model entries:
   - advertise and price `gpt-5.5`
   - add `gemini-3.5-flash` or approve fallback to existing `gemini-3-flash-preview` / `gemini-3.1-flash-lite`
   - confirm `gemini-3.1-pro-preview` vs `gemini-3.1-pro` product alias
3. Decide object-storage retention for uploaded student images.
4. Decide whether search goes through Search Center service APIs or directly through router `/v1/workflows/search`; Search Center is preferred because it owns history/cache.
5. Turn this material into project-local MissionD SSOT and scaffolding.
