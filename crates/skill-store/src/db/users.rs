use crate::models::{User, UserRole};
use rusqlite::{params, Connection};

pub fn create_user(
    conn: &Connection,
    id: &str,
    username: &str,
    email: &str,
    password_hash: &str,
) -> Result<User, rusqlite::Error> {
    conn.execute(
        "INSERT INTO users (id, username, email, password_hash) VALUES (?1, ?2, ?3, ?4)",
        params![id, username, email, password_hash],
    )?;
    get_user_by_id(conn, id)
}

pub fn get_user_by_id(conn: &Connection, id: &str) -> Result<User, rusqlite::Error> {
    conn.query_row(
        "SELECT id, username, email, password_hash, role, balance, created_at FROM users WHERE id = ?1",
        [id],
        |row| {
            Ok(User {
                id: row.get(0)?,
                username: row.get(1)?,
                email: row.get(2)?,
                password_hash: row.get(3)?,
                role: UserRole::from_str(&row.get::<_, String>(4)?),
                balance: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )
}

pub fn get_user_by_email(conn: &Connection, email: &str) -> Result<User, rusqlite::Error> {
    conn.query_row(
        "SELECT id, username, email, password_hash, role, balance, created_at FROM users WHERE email = ?1",
        [email],
        |row| {
            Ok(User {
                id: row.get(0)?,
                username: row.get(1)?,
                email: row.get(2)?,
                password_hash: row.get(3)?,
                role: UserRole::from_str(&row.get::<_, String>(4)?),
                balance: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )
}

pub fn update_role(
    conn: &Connection,
    user_id: &str,
    role: &UserRole,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE users SET role = ?1 WHERE id = ?2",
        params![role.as_str(), user_id],
    )?;
    Ok(())
}

pub fn update_balance(
    conn: &Connection,
    user_id: &str,
    delta: f64,
) -> Result<f64, rusqlite::Error> {
    conn.execute(
        "UPDATE users SET balance = balance + ?1 WHERE id = ?2",
        params![delta, user_id],
    )?;
    conn.query_row("SELECT balance FROM users WHERE id = ?1", [user_id], |r| {
        r.get(0)
    })
}
