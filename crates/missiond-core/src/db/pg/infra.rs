//! PostgreSQL impl of InfraStore.
//!
//! Stage 2B.1: empty shell. Methods will be migrated from ObservabilityStore
//! and SlotStore in Stage 2D.

use async_trait::async_trait;

use super::PgMissionStore;
use crate::db::traits::InfraStore;

#[async_trait]
impl InfraStore for PgMissionStore {
    // Stage 2D will migrate methods here.
}
