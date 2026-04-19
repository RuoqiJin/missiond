-- Directive / Plan / Workflow — user-directive compilation + plan DAG + workflow templates.
--
-- directive: user utterance -> lisp-coded directive (per dispatch).
--            语义: Jarvis 接到用户的话, 先生成 lisp 指令对齐意图, 再编译.
-- plan:      sexp execution DAG compiled from directive, pinned to a board_task.
-- workflow:  reusable template distilled from a successful plan.
--
-- NOTE: board_tasks.id is TEXT (see 20260318000000_init.sql), not UUID.
-- plan.board_task_id follows suit as TEXT to satisfy the FK.

CREATE TABLE IF NOT EXISTS directive (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    utterance_text  TEXT NOT NULL,
    sexp_text       TEXT NOT NULL,
    version         INT NOT NULL,
    status          TEXT NOT NULL,        -- draft|refining|approved|compiled|archived
    compiler_model  TEXT,
    references_json JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    approved_at     TIMESTAMPTZ,
    UNIQUE (id, version)
);

CREATE TABLE IF NOT EXISTS plan (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    board_task_id       TEXT NOT NULL REFERENCES board_tasks(id) ON DELETE CASCADE,
    source_directive_id UUID REFERENCES directive(id),
    version             INT NOT NULL,
    sexp_text           TEXT NOT NULL,
    sexp_hash           TEXT NOT NULL,
    status              TEXT NOT NULL,       -- draft|awaiting_approval|approved|executing|succeeded|failed|superseded
    compiler_model      TEXT,
    compiled_from       TEXT,                -- source directive sexp_hash
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    approved_at         TIMESTAMPTZ,
    finished_at         TIMESTAMPTZ,
    UNIQUE (board_task_id, version)
);

CREATE TABLE IF NOT EXISTS workflow (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT UNIQUE NOT NULL,
    sexp_text       TEXT NOT NULL,
    match_rules     JSONB NOT NULL,
    learned_from    UUID REFERENCES plan(id),
    executions      INT NOT NULL DEFAULT 0,
    success_count   INT NOT NULL DEFAULT 0,
    avg_cost_usd    NUMERIC(10,4),
    last_used_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- board_tasks reverse pointer to originating directive.
ALTER TABLE board_tasks ADD COLUMN IF NOT EXISTS source_directive_id UUID REFERENCES directive(id);

CREATE INDEX IF NOT EXISTS idx_directive_status ON directive(status);
CREATE INDEX IF NOT EXISTS idx_plan_board_task ON plan(board_task_id);
CREATE INDEX IF NOT EXISTS idx_plan_status ON plan(status);
CREATE INDEX IF NOT EXISTS idx_workflow_name ON workflow(name);
