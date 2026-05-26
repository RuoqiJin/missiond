use crate::models::{Skill, SkillPublic};
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

pub async fn create_skill(
    pool: &PgPool,
    id: &str,
    creator_id: &str,
    name: &str,
    description: &str,
    prompt_template: &str,
    input_schema: &serde_json::Value,
    tier: i32,
    price_per_use: f64,
) -> Result<Skill, sqlx::Error> {
    sqlx::query(
        "INSERT INTO skills (id, creator_id, name, description, prompt_template, input_schema, tier, price_per_use)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(creator_id)
    .bind(name)
    .bind(description)
    .bind(prompt_template)
    .bind(input_schema)
    .bind(tier)
    .bind(price_per_use)
    .execute(pool)
    .await?;
    get_skill_by_id(pool, id).await
}

pub async fn get_skill_by_id(pool: &PgPool, id: &str) -> Result<Skill, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, creator_id, name, description, prompt_template, input_schema, tier, price_per_use, is_active, invoke_count, created_at, updated_at
         FROM skills WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    row_to_skill(&row)
}

pub async fn list_skills_public(
    pool: &PgPool,
    tier_max: i32,
    offset: i64,
    limit: i64,
) -> Result<Vec<SkillPublic>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT s.id, s.creator_id, u.username AS creator_name, s.name, s.description, s.input_schema, s.tier, s.price_per_use, s.invoke_count, s.created_at
         FROM skills s JOIN users u ON s.creator_id = u.id
         WHERE s.is_active = true AND s.tier <= $1
         ORDER BY s.invoke_count DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(tier_max)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    rows.iter().map(row_to_skill_public).collect()
}

pub async fn list_creator_skills(
    pool: &PgPool,
    creator_id: &str,
) -> Result<Vec<Skill>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, creator_id, name, description, prompt_template, input_schema, tier, price_per_use, is_active, invoke_count, created_at, updated_at
         FROM skills WHERE creator_id = $1 ORDER BY created_at DESC",
    )
    .bind(creator_id)
    .fetch_all(pool)
    .await?;

    rows.iter().map(row_to_skill).collect()
}

pub async fn update_skill(
    pool: &PgPool,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
    prompt_template: Option<&str>,
    input_schema: Option<&serde_json::Value>,
    tier: Option<i32>,
    price_per_use: Option<f64>,
    is_active: Option<bool>,
) -> Result<(), sqlx::Error> {
    let mut builder = QueryBuilder::<Postgres>::new("UPDATE skills SET updated_at = now()");

    if let Some(v) = name {
        builder.push(", name = ").push_bind(v);
    }
    if let Some(v) = description {
        builder.push(", description = ").push_bind(v);
    }
    if let Some(v) = prompt_template {
        builder.push(", prompt_template = ").push_bind(v);
    }
    if let Some(v) = input_schema {
        builder.push(", input_schema = ").push_bind(v);
    }
    if let Some(v) = tier {
        builder.push(", tier = ").push_bind(v);
    }
    if let Some(v) = price_per_use {
        builder.push(", price_per_use = ").push_bind(v);
    }
    if let Some(v) = is_active {
        builder.push(", is_active = ").push_bind(v);
    }

    builder.push(" WHERE id = ").push_bind(id);
    builder.build().execute(pool).await?;
    Ok(())
}

pub async fn increment_invoke_count(pool: &PgPool, skill_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE skills SET invoke_count = invoke_count + 1 WHERE id = $1")
        .bind(skill_id)
        .execute(pool)
        .await?;
    Ok(())
}

fn row_to_skill(row: &sqlx::postgres::PgRow) -> Result<Skill, sqlx::Error> {
    Ok(Skill {
        id: row.try_get("id")?,
        creator_id: row.try_get("creator_id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        prompt_template: row.try_get("prompt_template")?,
        input_schema: row.try_get("input_schema")?,
        tier: row.try_get("tier")?,
        price_per_use: row.try_get("price_per_use")?,
        is_active: row.try_get("is_active")?,
        invoke_count: row.try_get("invoke_count")?,
        created_at: crate::db::ts_string(row, "created_at")?,
        updated_at: crate::db::ts_string(row, "updated_at")?,
    })
}

fn row_to_skill_public(row: &sqlx::postgres::PgRow) -> Result<SkillPublic, sqlx::Error> {
    Ok(SkillPublic {
        id: row.try_get("id")?,
        creator_id: row.try_get("creator_id")?,
        creator_name: row.try_get("creator_name")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        input_schema: row.try_get("input_schema")?,
        tier: row.try_get("tier")?,
        price_per_use: row.try_get("price_per_use")?,
        invoke_count: row.try_get("invoke_count")?,
        created_at: crate::db::ts_string(row, "created_at")?,
    })
}
