//! Project registry — domain module entry (D001 per memory pillar v0.4.23).
//!
//! Module `project-management` owns the `projects` table plus the four
//! `skill_*` tables. See:
//!   - Type: `crate::types::ProjectConfig`
//!   - Trait: `crate::db::traits::ProjectStore`
//!   - PG impl + helpers: `crate::db::pg::{skill, project}`
//!   - SQLite stub: `crate::db::sqlite::skill`
//!
//! The `projects` table is PG-only (SQLite is migration-only and returns
//! fail-safe empty stubs). This file intentionally contains no MissionDB
//! (SQLite) impl block because there is no `projects` table schema on the
//! SQLite side — all row access flows through the PG pool via
//! `pg::project::*` helpers invoked by the unified `ProjectStore` impl.

#[allow(unused_imports)]
pub use crate::types::ProjectConfig;
