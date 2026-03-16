pub mod migrations;
pub mod users;
pub mod skills;
pub mod subscriptions;
pub mod executions;

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Thread-safe database wrapper.
/// rusqlite::Connection is Send but not Sync, so we wrap in Mutex.
pub struct Database {
    conn: Mutex<Connection>,
    path: PathBuf,
}

// Safety: Connection is Send, and we protect it with Mutex
unsafe impl Sync for Database {}

impl Database {
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        migrations::run(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        })
    }

    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("DB lock poisoned")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
