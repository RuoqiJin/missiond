use sqlx::PgPool;

pub async fn run(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::raw_sql(MIGRATION_001).execute(pool).await?;
    Ok(())
}

const MIGRATION_001: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT UNIQUE NOT NULL,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'user',
    balance DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS plans (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    monthly_quota BIGINT NOT NULL,
    max_skill_tier INTEGER NOT NULL DEFAULT 1,
    price DOUBLE PRECISION NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT true
);

INSERT INTO plans (id, name, monthly_quota, max_skill_tier, price, is_active)
VALUES
    ('plan-free',  'Free',  50,   1, 0.0,   true),
    ('plan-basic', 'Basic', 500,  2, 9.99,  true),
    ('plan-pro',   'Pro',   2000, 3, 29.99, true),
    ('plan-max',   'Max',   10000,3, 99.99, true)
ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS subscriptions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    plan_id TEXT NOT NULL REFERENCES plans(id),
    status TEXT NOT NULL DEFAULT 'active',
    current_period_start TEXT NOT NULL,
    current_period_end TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_sub_user ON subscriptions(user_id, status);

CREATE TABLE IF NOT EXISTS skills (
    id TEXT PRIMARY KEY,
    creator_id TEXT NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    prompt_template TEXT NOT NULL,
    input_schema JSONB NOT NULL DEFAULT '{}'::jsonb,
    tier INTEGER NOT NULL DEFAULT 1,
    price_per_use DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    is_active BOOLEAN NOT NULL DEFAULT true,
    invoke_count BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_skills_creator ON skills(creator_id);
CREATE INDEX IF NOT EXISTS idx_skills_tier ON skills(tier, is_active);

CREATE TABLE IF NOT EXISTS skill_defense (
    id TEXT PRIMARY KEY,
    skill_id TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    position TEXT NOT NULL DEFAULT 'head',
    instruction TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS executions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    skill_id TEXT NOT NULL REFERENCES skills(id),
    status TEXT NOT NULL,
    input_tokens BIGINT NOT NULL DEFAULT 0,
    output_tokens BIGINT NOT NULL DEFAULT 0,
    cost DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    creator_revenue DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_exec_user ON executions(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_exec_skill ON executions(skill_id, created_at);

CREATE TABLE IF NOT EXISTS quota_usage (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    period_start TEXT NOT NULL,
    period_end TEXT NOT NULL,
    used BIGINT NOT NULL DEFAULT 0,
    extra_purchased BIGINT NOT NULL DEFAULT 0,
    UNIQUE(user_id, period_start)
);
CREATE INDEX IF NOT EXISTS idx_quota_user ON quota_usage(user_id, period_start);

CREATE TABLE IF NOT EXISTS revenue_ledger (
    id TEXT PRIMARY KEY,
    creator_id TEXT NOT NULL REFERENCES users(id),
    execution_id TEXT NOT NULL REFERENCES executions(id),
    amount DOUBLE PRECISION NOT NULL,
    settled BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_revenue_creator ON revenue_ledger(creator_id, settled);
"#;
