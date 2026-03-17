use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::{SkillStore, BeaconInfo, BeaconNode, AstSyncResult, AstSearchHit, AstNodeRow, AstStats, ModuleAstSummary, CodeNode};
use crate::types::*;

#[async_trait]
impl SkillStore for SqliteMissionStore {
    // ── skill.rs: topics ──────────────────────────────────────────

    async fn skill_topic_upsert(&self, topic: &str, description: Option<&str>, aka: Option<&str>, allowed_tools: Option<&str>, file_path: &str, requires_json: Option<&str>, actions_json: Option<&str>) -> DbResult<()> {
        let topic = topic.to_owned();
        let description = description.map(|s| s.to_owned());
        let aka = aka.map(|s| s.to_owned());
        let allowed_tools = allowed_tools.map(|s| s.to_owned());
        let file_path = file_path.to_owned();
        let requires_json = requires_json.map(|s| s.to_owned());
        let actions_json = actions_json.map(|s| s.to_owned());
        self.executor.run(move |db| db.skill_topic_upsert(&topic, description.as_deref(), aka.as_deref(), allowed_tools.as_deref(), &file_path, requires_json.as_deref(), actions_json.as_deref())).await
    }

    async fn skill_topic_upsert_full(&self, topic: &str, description: Option<&str>, aka: Option<&str>, allowed_tools: Option<&str>, file_path: &str, requires_json: Option<&str>, actions_json: Option<&str>, context_hooks_json: Option<&str>) -> DbResult<()> {
        let topic = topic.to_owned();
        let description = description.map(|s| s.to_owned());
        let aka = aka.map(|s| s.to_owned());
        let allowed_tools = allowed_tools.map(|s| s.to_owned());
        let file_path = file_path.to_owned();
        let requires_json = requires_json.map(|s| s.to_owned());
        let actions_json = actions_json.map(|s| s.to_owned());
        let context_hooks_json = context_hooks_json.map(|s| s.to_owned());
        self.executor.run(move |db| db.skill_topic_upsert_full(&topic, description.as_deref(), aka.as_deref(), allowed_tools.as_deref(), &file_path, requires_json.as_deref(), actions_json.as_deref(), context_hooks_json.as_deref())).await
    }

    async fn skill_topic_get(&self, topic: &str) -> DbResult<Option<SkillTopic>> {
        let topic = topic.to_owned();
        self.executor.run(move |db| db.skill_topic_get(&topic)).await
    }

    async fn skill_topic_list(&self) -> DbResult<Vec<SkillTopic>> {
        self.executor.run(|db| db.skill_topic_list()).await
    }

    async fn skill_topic_hit(&self, topic: &str) -> DbResult<()> {
        let topic = topic.to_owned();
        self.executor.run(move |db| db.skill_topic_hit(&topic)).await
    }

    async fn skill_topic_update_stats(&self, topic: &str, total_lines: i32, checksum: &str) -> DbResult<()> {
        let topic = topic.to_owned();
        let checksum = checksum.to_owned();
        self.executor.run(move |db| db.skill_topic_update_stats(&topic, total_lines, &checksum)).await
    }

    // ── skill.rs: blocks ──────────────────────────────────────────

    async fn skill_block_insert(&self, topic: &str, block_type: &str, title: Option<&str>, content: &str, sort_order: i32) -> DbResult<String> {
        let topic = topic.to_owned();
        let block_type = block_type.to_owned();
        let title = title.map(|s| s.to_owned());
        let content = content.to_owned();
        self.executor.run(move |db| db.skill_block_insert(&topic, &block_type, title.as_deref(), &content, sort_order)).await
    }

    async fn skill_block_update(&self, id: &str, content: &str) -> DbResult<bool> {
        let id = id.to_owned();
        let content = content.to_owned();
        self.executor.run(move |db| db.skill_block_update(&id, &content)).await
    }

    async fn skill_blocks_for_topic(&self, topic: &str) -> DbResult<Vec<SkillBlock>> {
        let topic = topic.to_owned();
        self.executor.run(move |db| db.skill_blocks_for_topic(&topic)).await
    }

    async fn skill_block_set_status(&self, id: &str, status: &str) -> DbResult<bool> {
        let id = id.to_owned();
        let status = status.to_owned();
        self.executor.run(move |db| db.skill_block_set_status(&id, &status)).await
    }

    async fn skill_blocks_delete_topic(&self, topic: &str) -> DbResult<usize> {
        let topic = topic.to_owned();
        self.executor.run(move |db| db.skill_blocks_delete_topic(&topic)).await
    }

    // ── skill.rs: search ──────────────────────────────────────────

    async fn skill_search_fts(&self, query: &str) -> DbResult<Vec<SkillSearchResult>> {
        let query = query.to_owned();
        self.executor.run(move |db| db.skill_search_fts(&query)).await
    }

    async fn skill_search_topics(&self, query: &str) -> DbResult<Vec<SkillTopic>> {
        let query = query.to_owned();
        self.executor.run(move |db| db.skill_search_topics(&query)).await
    }

    async fn skill_rebuild_fts(&self) -> DbResult<()> {
        self.executor.run(|db| db.skill_rebuild_fts()).await
    }

    // ── skill.rs: embeddings ──────────────────────────────────────

    async fn skill_set_topic_embedding(&self, topic: &str, embedding: &[f32], provider: &str) -> DbResult<()> {
        let topic = topic.to_owned();
        let embedding = embedding.to_vec();
        let provider = provider.to_owned();
        self.executor.run(move |db| db.skill_set_topic_embedding(&topic, &embedding, &provider)).await
    }

    async fn skill_load_topic_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>> {
        self.executor.run(|db| db.skill_load_topic_embeddings()).await
    }

    async fn skill_topics_missing_embedding(&self, limit: i64) -> DbResult<Vec<(String, String)>> {
        self.executor.run(move |db| db.skill_topics_missing_embedding(limit)).await
    }

    async fn skill_topics_stale_embedding(&self, current_provider: &str, limit: i64) -> DbResult<Vec<(String, String)>> {
        let current_provider = current_provider.to_owned();
        self.executor.run(move |db| db.skill_topics_stale_embedding(&current_provider, limit)).await
    }

    // ── skill.rs: executions ──────────────────────────────────────

    async fn skill_execution_insert(&self, id: &str, skill_topic: &str, action_id: &str, steps_total: i32, triggered_by: &str) -> DbResult<()> {
        let id = id.to_owned();
        let skill_topic = skill_topic.to_owned();
        let action_id = action_id.to_owned();
        let triggered_by = triggered_by.to_owned();
        self.executor.run(move |db| db.skill_execution_insert(&id, &skill_topic, &action_id, steps_total, &triggered_by)).await
    }

    async fn skill_execution_update(&self, id: &str, status: &str, steps_completed: i32, context_json: Option<&str>, error: Option<&str>) -> DbResult<()> {
        let id = id.to_owned();
        let status = status.to_owned();
        let context_json = context_json.map(|s| s.to_owned());
        let error = error.map(|s| s.to_owned());
        self.executor.run(move |db| db.skill_execution_update(&id, &status, steps_completed, context_json.as_deref(), error.as_deref())).await
    }

    async fn skill_execution_update_with_duration(&self, id: &str, status: &str, steps_completed: i32, context_json: Option<&str>, error: Option<&str>, duration_ms: Option<i64>) -> DbResult<()> {
        let id = id.to_owned();
        let status = status.to_owned();
        let context_json = context_json.map(|s| s.to_owned());
        let error = error.map(|s| s.to_owned());
        self.executor.run(move |db| db.skill_execution_update_with_duration(&id, &status, steps_completed, context_json.as_deref(), error.as_deref(), duration_ms)).await
    }

    async fn skill_execution_stats(&self, topic: Option<&str>) -> DbResult<Vec<SkillExecutionStat>> {
        let topic = topic.map(|s| s.to_owned());
        self.executor.run(move |db| db.skill_execution_stats(topic.as_deref())).await
    }

    async fn skill_execution_is_running(&self, skill_topic: &str, action_id: &str) -> DbResult<bool> {
        let skill_topic = skill_topic.to_owned();
        let action_id = action_id.to_owned();
        self.executor.run(move |db| db.skill_execution_is_running(&skill_topic, &action_id)).await
    }

    // ── skill.rs: versions ────────────────────────────────────────

    async fn skill_version_save(&self, topic: &str, content: &str, checksum: &str) -> DbResult<()> {
        let topic = topic.to_owned();
        let content = content.to_owned();
        let checksum = checksum.to_owned();
        self.executor.run(move |db| db.skill_version_save(&topic, &content, &checksum)).await
    }

    async fn skill_version_list(&self, topic: &str, limit: i64) -> DbResult<Vec<SkillVersion>> {
        let topic = topic.to_owned();
        self.executor.run(move |db| db.skill_version_list(&topic, limit)).await
    }

    async fn skill_version_get(&self, id: i64) -> DbResult<Option<SkillVersion>> {
        self.executor.run(move |db| db.skill_version_get(id)).await
    }

    // ── beacon.rs ─────────────────────────────────────────────────

    async fn beacon_list(&self) -> DbResult<Vec<BeaconInfo>> {
        self.executor.run(|db| db.beacon_list()).await
    }

    async fn beacon_ensure(&self, name: &str) -> DbResult<String> {
        let name = name.to_owned();
        self.executor.run(move |db| db.beacon_ensure(&name)).await
    }

    async fn beacon_increment_harvest(&self, name: &str) -> DbResult<i64> {
        let name = name.to_owned();
        self.executor.run(move |db| db.beacon_increment_harvest(&name)).await
    }

    async fn beacon_set_description(&self, name: &str, description: &str) -> DbResult<bool> {
        let name = name.to_owned();
        let description = description.to_owned();
        self.executor.run(move |db| db.beacon_set_description(&name, &description)).await
    }

    async fn beacon_node_upsert(&self, beacon_id: &str, repo: &str, file_path: &str, symbol_name: &str, annotation: Option<&str>) -> DbResult<()> {
        let beacon_id = beacon_id.to_owned();
        let repo = repo.to_owned();
        let file_path = file_path.to_owned();
        let symbol_name = symbol_name.to_owned();
        let annotation = annotation.map(|s| s.to_owned());
        self.executor.run(move |db| db.beacon_node_upsert(&beacon_id, &repo, &file_path, &symbol_name, annotation.as_deref())).await
    }

    async fn beacon_map(&self, beacon_name: &str) -> DbResult<Vec<BeaconNode>> {
        let beacon_name = beacon_name.to_owned();
        self.executor.run(move |db| db.beacon_map(&beacon_name)).await
    }

    async fn beacon_nodes_delete_file(&self, repo: &str, file_path: &str) -> DbResult<usize> {
        let repo = repo.to_owned();
        let file_path = file_path.to_owned();
        self.executor.run(move |db| db.beacon_nodes_delete_file(&repo, &file_path)).await
    }

    async fn beacon_node_annotate(&self, beacon_name: &str, repo: &str, file_path: &str, symbol_name: &str, annotation: &str) -> DbResult<bool> {
        let beacon_name = beacon_name.to_owned();
        let repo = repo.to_owned();
        let file_path = file_path.to_owned();
        let symbol_name = symbol_name.to_owned();
        let annotation = annotation.to_owned();
        self.executor.run(move |db| db.beacon_node_annotate(&beacon_name, &repo, &file_path, &symbol_name, &annotation)).await
    }

    async fn beacon_search(&self, query: &str) -> DbResult<Vec<BeaconInfo>> {
        let query = query.to_owned();
        self.executor.run(move |db| db.beacon_search(&query)).await
    }

    async fn beacon_cleanup_empty(&self) -> DbResult<usize> {
        self.executor.run(|db| db.beacon_cleanup_empty()).await
    }

    // ── ast.rs ────────────────────────────────────────────────────

    async fn ast_sync_file(&self, repo: &str, file_path: &str, commit_hash: &str, nodes: &[CodeNode]) -> DbResult<AstSyncResult> {
        let repo = repo.to_owned();
        let file_path = file_path.to_owned();
        let commit_hash = commit_hash.to_owned();
        let nodes = nodes.to_vec();
        self.executor.run(move |db| db.ast_sync_file(&repo, &file_path, &commit_hash, &nodes)).await
    }

    async fn ast_delete_file(&self, repo: &str, file_path: &str) -> DbResult<usize> {
        let repo = repo.to_owned();
        let file_path = file_path.to_owned();
        self.executor.run(move |db| db.ast_delete_file(&repo, &file_path)).await
    }

    async fn ast_search(&self, query: &str, limit: usize) -> DbResult<Vec<AstSearchHit>> {
        let query = query.to_owned();
        self.executor.run(move |db| db.ast_search(&query, limit)).await
    }

    async fn ast_get_file_nodes(&self, repo: &str, file_path: &str) -> DbResult<Vec<AstNodeRow>> {
        let repo = repo.to_owned();
        let file_path = file_path.to_owned();
        self.executor.run(move |db| db.ast_get_file_nodes(&repo, &file_path)).await
    }

    async fn ast_find_related(&self, name: &str, limit: usize) -> DbResult<Vec<AstSearchHit>> {
        let name = name.to_owned();
        self.executor.run(move |db| db.ast_find_related(&name, limit)).await
    }

    async fn ast_get_file_commit(&self, repo: &str, file_path: &str) -> DbResult<Option<String>> {
        let repo = repo.to_owned();
        let file_path = file_path.to_owned();
        self.executor.run(move |db| db.ast_get_file_commit(&repo, &file_path)).await
    }

    async fn ast_stats(&self) -> DbResult<AstStats> {
        self.executor.run(|db| db.ast_stats()).await
    }

    async fn ast_set_embedding(&self, node_id: &str, embedding: &[u8], provider: &str) -> DbResult<()> {
        let node_id = node_id.to_owned();
        let embedding = embedding.to_vec();
        let provider = provider.to_owned();
        self.executor.run(move |db| db.ast_set_embedding(&node_id, &embedding, &provider)).await
    }

    async fn ast_find_unembedded(&self, limit: usize) -> DbResult<Vec<(String, String, String)>> {
        self.executor.run(move |db| db.ast_find_unembedded(limit)).await
    }

    async fn ast_load_all_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>> {
        self.executor.run(|db| db.ast_load_all_embeddings()).await
    }

    async fn ast_search_ranked(&self, query: &str, limit: usize) -> DbResult<Vec<(String, usize)>> {
        let query = query.to_owned();
        self.executor.run(move |db| db.ast_search_ranked(&query, limit)).await
    }

    async fn ast_get_search_hit(&self, node_id: &str) -> DbResult<Option<AstSearchHit>> {
        let node_id = node_id.to_owned();
        self.executor.run(move |db| db.ast_get_search_hit(&node_id)).await
    }

    async fn ast_get_node(&self, node_id: &str) -> DbResult<Option<AstNodeRow>> {
        let node_id = node_id.to_owned();
        self.executor.run(move |db| db.ast_get_node(&node_id)).await
    }

    async fn ast_module_summaries(&self, repo: &str) -> DbResult<Vec<ModuleAstSummary>> {
        let repo = repo.to_owned();
        self.executor.run(move |db| db.ast_module_summaries(&repo)).await
    }
}
