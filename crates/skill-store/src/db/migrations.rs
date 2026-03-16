use rusqlite::Connection;

pub fn run(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        );
        ",
    )?;

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let migrations: Vec<(i64, &str)> = vec![(1, MIGRATION_001)];

    for (ver, sql) in &migrations {
        if *ver > current {
            tracing::info!("Running migration v{ver}");
            conn.execute_batch(sql)?;
            conn.execute("INSERT INTO schema_version (version) VALUES (?1)", [ver])?;
        }
    }

    Ok(())
}

const MIGRATION_001: &str = "
-- Users
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT UNIQUE NOT NULL,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'user',
    balance REAL NOT NULL DEFAULT 0.0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Subscription plans
CREATE TABLE IF NOT EXISTS plans (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    monthly_quota INTEGER NOT NULL,
    max_skill_tier INTEGER NOT NULL DEFAULT 1,
    price REAL NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 1
);

-- Seed default plans
INSERT OR IGNORE INTO plans (id, name, monthly_quota, max_skill_tier, price, is_active)
VALUES
    ('plan-free',  'Free',  50,   1, 0.0,   1),
    ('plan-basic', 'Basic', 500,  2, 9.99,  1),
    ('plan-pro',   'Pro',   2000, 3, 29.99, 1),
    ('plan-max',   'Max',   10000,3, 99.99, 1);

-- User subscriptions
CREATE TABLE IF NOT EXISTS subscriptions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    plan_id TEXT NOT NULL REFERENCES plans(id),
    status TEXT NOT NULL DEFAULT 'active',
    current_period_start TEXT NOT NULL,
    current_period_end TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_sub_user ON subscriptions(user_id, status);

-- Skills
CREATE TABLE IF NOT EXISTS skills (
    id TEXT PRIMARY KEY,
    creator_id TEXT NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    prompt_template TEXT NOT NULL,
    input_schema TEXT NOT NULL DEFAULT '{}',
    tier INTEGER NOT NULL DEFAULT 1,
    price_per_use REAL NOT NULL DEFAULT 0.0,
    is_active INTEGER NOT NULL DEFAULT 1,
    invoke_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_skills_creator ON skills(creator_id);
CREATE INDEX IF NOT EXISTS idx_skills_tier ON skills(tier, is_active);

-- Skill defense instructions (injected at head/tail of prompt)
CREATE TABLE IF NOT EXISTS skill_defense (
    id TEXT PRIMARY KEY,
    skill_id TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    position TEXT NOT NULL DEFAULT 'head',
    instruction TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Execution log
CREATE TABLE IF NOT EXISTS executions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    skill_id TEXT NOT NULL REFERENCES skills(id),
    status TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cost REAL NOT NULL DEFAULT 0.0,
    creator_revenue REAL NOT NULL DEFAULT 0.0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_exec_user ON executions(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_exec_skill ON executions(skill_id, created_at);

-- Quota tracking (per user per billing period)
CREATE TABLE IF NOT EXISTS quota_usage (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    period_start TEXT NOT NULL,
    period_end TEXT NOT NULL,
    used INTEGER NOT NULL DEFAULT 0,
    extra_purchased INTEGER NOT NULL DEFAULT 0,
    UNIQUE(user_id, period_start)
);
CREATE INDEX IF NOT EXISTS idx_quota_user ON quota_usage(user_id, period_start);

-- Creator revenue ledger
CREATE TABLE IF NOT EXISTS revenue_ledger (
    id TEXT PRIMARY KEY,
    creator_id TEXT NOT NULL REFERENCES users(id),
    execution_id TEXT NOT NULL REFERENCES executions(id),
    amount REAL NOT NULL,
    settled INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_revenue_creator ON revenue_ledger(creator_id, settled);
";
