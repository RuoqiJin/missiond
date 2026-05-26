use crate::models::{User, UserRole};
use sqlx::{PgPool, Row};

pub async fn create_user(
    pool: &PgPool,
    id: &str,
    username: &str,
    email: &str,
    password_hash: &str,
) -> Result<User, sqlx::Error> {
    sqlx::query("INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(username)
        .bind(email)
        .bind(password_hash)
        .execute(pool)
        .await?;
    get_user_by_id(pool, id).await
}

pub async fn get_user_by_id(pool: &PgPool, id: &str) -> Result<User, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, username, email, password_hash, role, balance, created_at FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    row_to_user(&row)
}

pub async fn get_user_by_email(pool: &PgPool, email: &str) -> Result<User, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, username, email, password_hash, role, balance, created_at FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_one(pool)
    .await?;
    row_to_user(&row)
}

pub async fn update_role(pool: &PgPool, user_id: &str, role: &UserRole) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET role = $1 WHERE id = $2")
        .bind(role.as_str())
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_balance(pool: &PgPool, user_id: &str, delta: f64) -> Result<f64, sqlx::Error> {
    let row =
        sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2 RETURNING balance")
            .bind(delta)
            .bind(user_id)
            .fetch_one(pool)
            .await?;
    row.try_get("balance")
}

fn row_to_user(row: &sqlx::postgres::PgRow) -> Result<User, sqlx::Error> {
    let role: String = row.try_get("role")?;
    Ok(User {
        id: row.try_get("id")?,
        username: row.try_get("username")?,
        email: row.try_get("email")?,
        password_hash: row.try_get("password_hash")?,
        role: UserRole::from_str(&role),
        balance: row.try_get("balance")?,
        created_at: crate::db::ts_string(row, "created_at")?,
    })
}
