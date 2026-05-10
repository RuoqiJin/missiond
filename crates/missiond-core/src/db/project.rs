//! Project registry — domain module entry (D001 per memory pillar v0.4.23).
//!
//! Module `project-management` owns the `projects` table plus the four
//! `skill_*` tables. See:
//!   - Type: `crate::types::ProjectConfig`
//!   - Trait: `crate::db::traits::ProjectStore`
//!   - PG impl + helpers: `crate::db::pg::{skill, project}`
//!
//! The `projects` table is PostgreSQL-only; all row access flows through the
//! PG pool via `pg::project::*` helpers invoked by the unified `ProjectStore`
//! impl.

#[allow(unused_imports)]
pub use crate::types::ProjectConfig;
