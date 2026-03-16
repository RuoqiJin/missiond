use crate::models::{Skill, SkillPublic};
use rusqlite::{params, Connection};

pub fn create_skill(
    conn: &Connection,
    id: &str,
    creator_id: &str,
    name: &str,
    description: &str,
    prompt_template: &str,
    input_schema: &serde_json::Value,
    tier: i32,
    price_per_use: f64,
) -> Result<Skill, rusqlite::Error> {
    let schema_str = serde_json::to_string(input_schema).unwrap_or_default();
    conn.execute(
        "INSERT INTO skills (id, creator_id, name, description, prompt_template, input_schema, tier, price_per_use)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, creator_id, name, description, prompt_template, schema_str, tier, price_per_use],
    )?;
    get_skill_by_id(conn, id)
}

pub fn get_skill_by_id(conn: &Connection, id: &str) -> Result<Skill, rusqlite::Error> {
    conn.query_row(
        "SELECT id, creator_id, name, description, prompt_template, input_schema, tier, price_per_use, is_active, invoke_count, created_at, updated_at
         FROM skills WHERE id = ?1",
        [id],
        row_to_skill,
    )
}

pub fn list_skills_public(conn: &Connection, tier_max: i32, offset: i64, limit: i64) -> Result<Vec<SkillPublic>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.creator_id, u.username, s.name, s.description, s.input_schema, s.tier, s.price_per_use, s.invoke_count, s.created_at
         FROM skills s JOIN users u ON s.creator_id = u.id
         WHERE s.is_active = 1 AND s.tier <= ?1
         ORDER BY s.invoke_count DESC
         LIMIT ?2 OFFSET ?3",
    )?;

    let rows = stmt.query_map(params![tier_max, limit, offset], |row| {
        let schema_str: String = row.get(5)?;
        Ok(SkillPublic {
            id: row.get(0)?,
            creator_id: row.get(1)?,
            creator_name: row.get(2)?,
            name: row.get(3)?,
            description: row.get(4)?,
            input_schema: serde_json::from_str(&schema_str).unwrap_or_default(),
            tier: row.get(6)?,
            price_per_use: row.get(7)?,
            invoke_count: row.get(8)?,
            created_at: row.get(9)?,
        })
    })?;

    rows.collect()
}

pub fn list_creator_skills(conn: &Connection, creator_id: &str) -> Result<Vec<Skill>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, creator_id, name, description, prompt_template, input_schema, tier, price_per_use, is_active, invoke_count, created_at, updated_at
         FROM skills WHERE creator_id = ?1 ORDER BY created_at DESC",
    )?;

    let rows = stmt.query_map([creator_id], row_to_skill)?;
    rows.collect()
}

pub fn update_skill(
    conn: &Connection,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
    prompt_template: Option<&str>,
    input_schema: Option<&serde_json::Value>,
    tier: Option<i32>,
    price_per_use: Option<f64>,
    is_active: Option<bool>,
) -> Result<(), rusqlite::Error> {
    let mut sets = vec!["updated_at = datetime('now')".to_string()];
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];

    if let Some(v) = name {
        sets.push(format!("name = ?{}", values.len() + 1));
        values.push(Box::new(v.to_string()));
    }
    if let Some(v) = description {
        sets.push(format!("description = ?{}", values.len() + 1));
        values.push(Box::new(v.to_string()));
    }
    if let Some(v) = prompt_template {
        sets.push(format!("prompt_template = ?{}", values.len() + 1));
        values.push(Box::new(v.to_string()));
    }
    if let Some(v) = input_schema {
        sets.push(format!("input_schema = ?{}", values.len() + 1));
        values.push(Box::new(serde_json::to_string(v).unwrap_or_default()));
    }
    if let Some(v) = tier {
        sets.push(format!("tier = ?{}", values.len() + 1));
        values.push(Box::new(v));
    }
    if let Some(v) = price_per_use {
        sets.push(format!("price_per_use = ?{}", values.len() + 1));
        values.push(Box::new(v));
    }
    if let Some(v) = is_active {
        sets.push(format!("is_active = ?{}", values.len() + 1));
        values.push(Box::new(v as i32));
    }

    let idx = values.len() + 1;
    values.push(Box::new(id.to_string()));

    let sql = format!("UPDATE skills SET {} WHERE id = ?{}", sets.join(", "), idx);
    let params: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    conn.execute(&sql, params.as_slice())?;
    Ok(())
}

pub fn increment_invoke_count(conn: &Connection, skill_id: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE skills SET invoke_count = invoke_count + 1 WHERE id = ?1",
        [skill_id],
    )?;
    Ok(())
}

fn row_to_skill(row: &rusqlite::Row) -> Result<Skill, rusqlite::Error> {
    let schema_str: String = row.get(5)?;
    Ok(Skill {
        id: row.get(0)?,
        creator_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        prompt_template: row.get(4)?,
        input_schema: serde_json::from_str(&schema_str).unwrap_or_default(),
        tier: row.get(6)?,
        price_per_use: row.get(7)?,
        is_active: row.get::<_, i32>(8)? != 0,
        invoke_count: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}
