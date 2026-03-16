use rusqlite::params;
use super::error::DbResult;
use super::MissionDB;
use crate::types::DynamicSlot;

impl MissionDB {
    /// Insert a new dynamic slot record.
    pub fn create_dynamic_slot(&self, slot: &DynamicSlot) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO dynamic_slots (id, parent_slot_id, template, objective, config, status, created_at, ttl_seconds, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                slot.id, slot.parent_slot_id, slot.template, slot.objective,
                slot.config, slot.status, slot.created_at, slot.ttl_seconds, slot.expires_at,
            ],
        )?;
        Ok(())
    }

    /// Get a dynamic slot by ID.
    pub fn get_dynamic_slot(&self, id: &str) -> DbResult<Option<DynamicSlot>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, parent_slot_id, template, objective, config, status, termination_reason,
                    created_at, terminated_at, ttl_seconds, expires_at, extend_count
             FROM dynamic_slots WHERE id = ?1"
        )?;
        let result = stmt.query_row(params![id], Self::row_to_dynamic_slot);
        match result {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// List all dynamic slots (optionally filtered by status).
    pub fn list_dynamic_slots(&self, status: Option<&str>) -> DbResult<Vec<DynamicSlot>> {
        let conn = self.read_conn();
        let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match status {
            Some(s) => (
                "SELECT id, parent_slot_id, template, objective, config, status, termination_reason,
                        created_at, terminated_at, ttl_seconds, expires_at, extend_count
                 FROM dynamic_slots WHERE status = ?1 ORDER BY created_at DESC".to_string(),
                vec![Box::new(s.to_string())],
            ),
            None => (
                "SELECT id, parent_slot_id, template, objective, config, status, termination_reason,
                        created_at, terminated_at, ttl_seconds, expires_at, extend_count
                 FROM dynamic_slots ORDER BY created_at DESC".to_string(),
                vec![],
            ),
        };
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_ref.as_slice(), Self::row_to_dynamic_slot)?;
        Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Count active dynamic slots.
    pub fn count_active_dynamic_slots(&self) -> DbResult<i64> {
        let conn = self.read_conn();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM dynamic_slots WHERE status = 'active'",
            [],
            |row| row.get(0),
        )?)
    }

    /// Terminate a dynamic slot.
    pub fn terminate_dynamic_slot(&self, id: &str, reason: &str) -> DbResult<bool> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let updated = conn.execute(
            "UPDATE dynamic_slots SET status = 'terminated', termination_reason = ?1, terminated_at = ?2
             WHERE id = ?3 AND status = 'active'",
            params![reason, now, id],
        )?;
        Ok(updated > 0)
    }

    /// Extend a dynamic slot's TTL. Returns new expires_at or None if not allowed.
    /// Enforces: max 2 extensions AND absolute cap of created_at + 8h.
    pub fn extend_dynamic_slot(&self, id: &str, additional_seconds: i64) -> DbResult<Option<String>> {
        let conn = self.conn();
        // Check current state
        let (current_expires, extend_count, created_at_str): (String, i64, String) = match conn.query_row(
            "SELECT expires_at, extend_count, created_at FROM dynamic_slots WHERE id = ?1 AND status = 'active'",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ) {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        if extend_count >= 2 {
            return Ok(None); // Max 2 extensions
        }

        // Calculate new expires_at
        let current_dt = chrono::DateTime::parse_from_rfc3339(&current_expires)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
        let new_expires = current_dt + chrono::Duration::seconds(additional_seconds);

        // Enforce absolute TTL cap: created_at + 8 hours (MAX_TTL_SECS)
        const MAX_TTL_SECS: i64 = 28800;
        if let Ok(created_dt) = chrono::DateTime::parse_from_rfc3339(&created_at_str) {
            let absolute_cap = created_dt + chrono::Duration::seconds(MAX_TTL_SECS);
            if new_expires > absolute_cap {
                return Ok(None); // Would exceed absolute 8h cap
            }
        }

        let new_expires_str = new_expires.to_rfc3339();

        conn.execute(
            "UPDATE dynamic_slots SET expires_at = ?1, extend_count = extend_count + 1
             WHERE id = ?2 AND status = 'active'",
            params![new_expires_str, id],
        )?;

        Ok(Some(new_expires_str))
    }

    /// Find expired active slots (for Reaper).
    pub fn find_expired_dynamic_slots(&self) -> DbResult<Vec<DynamicSlot>> {
        let conn = self.read_conn();
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT id, parent_slot_id, template, objective, config, status, termination_reason,
                    created_at, terminated_at, ttl_seconds, expires_at, extend_count
             FROM dynamic_slots WHERE status = 'active' AND expires_at <= ?1"
        )?;
        let rows = stmt.query_map(params![now], Self::row_to_dynamic_slot)?;
        Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Find slots expiring within N seconds (for TTL warning).
    pub fn find_expiring_dynamic_slots(&self, within_seconds: i64) -> DbResult<Vec<DynamicSlot>> {
        let conn = self.read_conn();
        let deadline = (chrono::Utc::now() + chrono::Duration::seconds(within_seconds)).to_rfc3339();
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT id, parent_slot_id, template, objective, config, status, termination_reason,
                    created_at, terminated_at, ttl_seconds, expires_at, extend_count
             FROM dynamic_slots WHERE status = 'active' AND expires_at > ?1 AND expires_at <= ?2"
        )?;
        let rows = stmt.query_map(params![now, deadline], Self::row_to_dynamic_slot)?;
        Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    fn row_to_dynamic_slot(row: &rusqlite::Row) -> rusqlite::Result<DynamicSlot> {
        Ok(DynamicSlot {
            id: row.get("id")?,
            parent_slot_id: row.get("parent_slot_id")?,
            template: row.get("template")?,
            objective: row.get("objective")?,
            config: row.get("config")?,
            status: row.get("status")?,
            termination_reason: row.get("termination_reason")?,
            created_at: row.get("created_at")?,
            terminated_at: row.get("terminated_at")?,
            ttl_seconds: row.get("ttl_seconds")?,
            expires_at: row.get("expires_at")?,
            extend_count: row.get("extend_count")?,
        })
    }
}
