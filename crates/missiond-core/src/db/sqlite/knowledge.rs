use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::KnowledgeStore;
use crate::types::*;
use std::collections::HashMap;

#[async_trait]
impl KnowledgeStore for SqliteMissionStore {
    // -- Core CRUD --

    async fn kb_remember(&self, input: &KBRememberInput) -> DbResult<KBRememberResult> {
        let input = input.clone();
        self.executor.run(move |db| db.kb_remember(&input)).await
    }

    async fn kb_get(&self, key: &str) -> DbResult<Option<KnowledgeEntry>> {
        let key = key.to_owned();
        self.executor.run(move |db| db.kb_get(&key)).await
    }

    async fn kb_get_by_id(&self, id: &str) -> DbResult<Option<KnowledgeEntry>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.kb_get_by_id(&id)).await
    }

    async fn kb_get_id_by_key(&self, key: &str) -> DbResult<Option<String>> {
        let key = key.to_owned();
        self.executor.run(move |db| db.kb_get_id_by_key(&key)).await
    }

    async fn kb_update(&self, key: &str, new_category: Option<&str>, new_summary: Option<&str>, new_detail: Option<&serde_json::Value>, new_confidence: Option<f64>, new_linked_task_id: Option<&str>) -> DbResult<Option<(KnowledgeEntry, bool)>> {
        let key = key.to_owned();
        let new_category = new_category.map(|s| s.to_owned());
        let new_summary = new_summary.map(|s| s.to_owned());
        let new_detail = new_detail.cloned();
        let new_linked_task_id = new_linked_task_id.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_update(&key, new_category.as_deref(), new_summary.as_deref(), new_detail.as_ref(), new_confidence, new_linked_task_id.as_deref())).await
    }

    async fn kb_set_linked_task_id(&self, key: &str, task_id: Option<&str>) -> DbResult<bool> {
        let key = key.to_owned();
        let task_id = task_id.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_set_linked_task_id(&key, task_id.as_deref())).await
    }

    async fn kb_forget(&self, key: &str) -> DbResult<bool> {
        let key = key.to_owned();
        self.executor.run(move |db| db.kb_forget(&key)).await
    }

    async fn kb_batch_forget(&self, keys: &[String]) -> DbResult<usize> {
        let keys = keys.to_vec();
        self.executor.run(move |db| db.kb_batch_forget(&keys)).await
    }

    async fn kb_list(&self, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>> {
        let category = category.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_list(category.as_deref())).await
    }

    async fn kb_list_paginated(&self, category: Option<&str>, limit: u32, offset: u32) -> DbResult<Vec<KnowledgeEntry>> {
        let category = category.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_list_paginated(category.as_deref(), limit, offset)).await
    }

    async fn kb_list_by_scope(&self, task_id: &str) -> DbResult<Vec<KnowledgeEntry>> {
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.kb_list_by_scope(&task_id)).await
    }

    async fn kb_clear_scope(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.kb_clear_scope(&id)).await
    }

    async fn kb_update_access_stats(&self, entries: &[KnowledgeEntry]) -> DbResult<()> {
        let entries = entries.to_vec();
        self.executor.run(move |db| db.kb_update_access_stats(&entries)).await
    }

    // -- Search --

    async fn kb_search(&self, query: &str, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>> {
        let query = query.to_owned();
        let category = category.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_search(&query, category.as_deref())).await
    }

    async fn kb_search_ranked(&self, query: &str, category: Option<&str>, limit: usize) -> DbResult<Vec<(KnowledgeEntry, usize)>> {
        let query = query.to_owned();
        let category = category.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_search_ranked(&query, category.as_deref(), limit)).await
    }

    async fn kb_search_fts_ranked(&self, query: &str, category: Option<&str>) -> DbResult<Vec<(String, usize, Option<String>)>> {
        let query = query.to_owned();
        let category = category.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_search_fts_ranked(&query, category.as_deref())).await
    }

    async fn kb_search_like_ranked(&self, query: &str, category: Option<&str>) -> DbResult<Vec<(String, usize)>> {
        let query = query.to_owned();
        let category = category.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_search_like_ranked(&query, category.as_deref())).await
    }

    // -- Embeddings --

    async fn kb_set_embedding(&self, id: &str, embedding: &[f32], provider: &str) -> DbResult<()> {
        let id = id.to_owned();
        let embedding = embedding.to_vec();
        let provider = provider.to_owned();
        self.executor.run(move |db| db.kb_set_embedding(&id, &embedding, &provider)).await
    }

    async fn kb_load_embeddings(&self, category: &str) -> DbResult<Vec<(String, Vec<f32>)>> {
        let category = category.to_owned();
        self.executor.run(move |db| db.kb_load_embeddings(&category)).await
    }

    async fn kb_load_all_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>> {
        self.executor.run(|db| db.kb_load_all_embeddings()).await
    }

    async fn kb_entries_missing_embedding(&self, category: Option<&str>) -> DbResult<Vec<(String, String, String)>> {
        let category = category.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_entries_missing_embedding(category.as_deref())).await
    }

    async fn kb_entries_stale_embedding(&self, current_provider: &str, limit: i64) -> DbResult<Vec<(String, String, String)>> {
        let current_provider = current_provider.to_owned();
        self.executor.run(move |db| db.kb_entries_stale_embedding(&current_provider, limit)).await
    }

    // -- Stats & GC --

    async fn kb_stats(&self) -> DbResult<serde_json::Value> {
        self.executor.run(|db| db.kb_stats()).await
    }

    async fn kb_summary(&self) -> DbResult<Vec<(String, i64)>> {
        self.executor.run(|db| db.kb_summary()).await
    }

    async fn kb_hot_keys(&self, limit: i64) -> DbResult<Vec<String>> {
        self.executor.run(move |db| db.kb_hot_keys(limit)).await
    }

    async fn kb_find_stale(&self, days: i64) -> DbResult<Vec<KnowledgeEntry>> {
        self.executor.run(move |db| db.kb_find_stale(days)).await
    }

    async fn kb_find_duplicates(&self) -> DbResult<Vec<(KnowledgeEntry, KnowledgeEntry, f64)>> {
        self.executor.run(|db| db.kb_find_duplicates()).await
    }

    async fn embedding_stats(&self) -> DbResult<serde_json::Value> {
        self.executor.run(|db| db.embedding_stats()).await
    }

    async fn kb_auto_gc(&self) -> DbResult<usize> {
        self.executor.run(|db| db.kb_auto_gc()).await
    }

    async fn kb_adjust_confidence(&self, id: &str, delta: f64) -> DbResult<Option<f64>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.kb_adjust_confidence(&id, delta)).await
    }

    async fn kb_batch_apply_utility_feedback(&self, kb_ids: &[String], success: bool) -> DbResult<usize> {
        let kb_ids = kb_ids.to_vec();
        self.executor.run(move |db| db.kb_batch_apply_utility_feedback(&kb_ids, success)).await
    }

    async fn kb_list_low_confidence(&self, threshold: f64, limit: usize) -> DbResult<Vec<KnowledgeEntry>> {
        self.executor.run(move |db| db.kb_list_low_confidence(threshold, limit)).await
    }

    async fn kb_list_low_utility(&self, threshold: f64, min_access: i64, limit: usize) -> DbResult<Vec<KnowledgeEntry>> {
        self.executor.run(move |db| db.kb_list_low_utility(threshold, min_access, limit)).await
    }

    async fn kb_mark_needs_re_extraction(&self, ids: &[String]) -> DbResult<usize> {
        let ids = ids.to_vec();
        self.executor.run(move |db| db.kb_mark_needs_re_extraction(&ids)).await
    }

    async fn kb_list_stale_state_entries(&self, stale_days: i64, limit: usize) -> DbResult<Vec<KnowledgeEntry>> {
        self.executor.run(move |db| db.kb_list_stale_state_entries(stale_days, limit)).await
    }

    // -- FTS management --

    async fn kb_rebuild_fts_if_dirty(&self) -> DbResult<bool> {
        self.executor.run(|db| db.kb_rebuild_fts_if_dirty()).await
    }

    // -- KB Operations queue --

    async fn kb_ops_save_plan(&self, plan_id: &str, task_id: Option<&str>, operations: &[KBOperation]) -> DbResult<usize> {
        let plan_id = plan_id.to_owned();
        let task_id = task_id.map(|s| s.to_owned());
        let operations = operations.to_vec();
        self.executor.run(move |db| db.kb_ops_save_plan(&plan_id, task_id.as_deref(), &operations)).await
    }

    async fn kb_ops_list(&self, plan_id: Option<&str>, status: Option<&str>) -> DbResult<Vec<KBOperationRow>> {
        let plan_id = plan_id.map(|s| s.to_owned());
        let status = status.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_ops_list(plan_id.as_deref(), status.as_deref())).await
    }

    async fn kb_ops_update_status(&self, op_id: &str, status: &str, result: Option<&str>, error: Option<&str>) -> DbResult<bool> {
        let op_id = op_id.to_owned();
        let status = status.to_owned();
        let result = result.map(|s| s.to_owned());
        let error = error.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_ops_update_status(&op_id, &status, result.as_deref(), error.as_deref())).await
    }

    async fn kb_ops_complete_by_task_id(&self, task_id: &str, new_status: &str, result: Option<&str>) -> DbResult<bool> {
        let task_id = task_id.to_owned();
        let new_status = new_status.to_owned();
        let result = result.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_ops_complete_by_task_id(&task_id, &new_status, result.as_deref())).await
    }

    async fn kb_ops_expire_stale(&self, ttl_secs: i64) -> DbResult<usize> {
        self.executor.run(move |db| db.kb_ops_expire_stale(ttl_secs)).await
    }

    async fn kb_ops_plan_summary(&self, plan_id: &str) -> DbResult<serde_json::Value> {
        let plan_id = plan_id.to_owned();
        self.executor.run(move |db| db.kb_ops_plan_summary(&plan_id)).await
    }

    // -- Knowledge Graph edges --

    async fn kb_add_edge(&self, source_id: &str, target_id: &str, relation_type: &str, weight: f64) -> DbResult<()> {
        let source_id = source_id.to_owned();
        let target_id = target_id.to_owned();
        let relation_type = relation_type.to_owned();
        self.executor.run(move |db| db.kb_add_edge(&source_id, &target_id, &relation_type, weight)).await
    }

    async fn kb_get_edges(&self, id: &str) -> DbResult<Vec<KBEdge>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.kb_get_edges(&id)).await
    }

    async fn kb_expand_related(&self, ids: &[String], max_extra: usize) -> DbResult<Vec<String>> {
        let ids = ids.to_vec();
        self.executor.run(move |db| db.kb_expand_related(&ids, max_extra)).await
    }

    async fn kb_delete_edges_for(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.kb_delete_edges_for(&id)).await
    }

    async fn kb_log_co_access(&self, kb_ids: &[String], source: &str, session_id: Option<&str>) -> DbResult<()> {
        let kb_ids = kb_ids.to_vec();
        let source = source.to_owned();
        let session_id = session_id.map(|s| s.to_owned());
        self.executor.run(move |db| db.kb_log_co_access(&kb_ids, &source, session_id.as_deref())).await
    }

    async fn get_session_cited_kb_ids(&self, session_id: &str) -> DbResult<Vec<String>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_session_cited_kb_ids(&session_id)).await
    }

    async fn kb_compute_cooccurrence(&self, since_hours: i64, top_n: usize) -> DbResult<HashMap<String, Vec<String>>> {
        self.executor.run(move |db| db.kb_compute_cooccurrence(since_hours, top_n)).await
    }

    // -- Prompt snapshots --

    async fn save_prompt_snapshot(&self, task_id: &str, prompt: &str, cited_kb_ids: &[String], category: &str) -> DbResult<()> {
        let task_id = task_id.to_owned();
        let prompt = prompt.to_owned();
        let cited_kb_ids = cited_kb_ids.to_vec();
        let category = category.to_owned();
        self.executor.run(move |db| db.save_prompt_snapshot(&task_id, &prompt, &cited_kb_ids, &category)).await
    }

    async fn update_prompt_snapshot_outcome(&self, task_id: &str, outcome: &str) -> DbResult<()> {
        let task_id = task_id.to_owned();
        let outcome = outcome.to_owned();
        self.executor.run(move |db| db.update_prompt_snapshot_outcome(&task_id, &outcome)).await
    }

    async fn list_prompt_snapshots(&self, category: &str, limit: usize) -> DbResult<Vec<(String, String, String)>> {
        let category = category.to_owned();
        self.executor.run(move |db| db.list_prompt_snapshots(&category, limit)).await
    }

    async fn list_modified_snapshots(&self, limit: usize) -> DbResult<Vec<(String, String, String, String)>> {
        self.executor.run(move |db| db.list_modified_snapshots(limit)).await
    }

    // -- AST links --

    async fn kb_add_ast_link(&self, kb_id: &str, symbol_name: &str, file_path: Option<&str>, ast_node_id: Option<&str>, relation: &str, confidence: f64) -> DbResult<()> {
        let kb_id = kb_id.to_owned();
        let symbol_name = symbol_name.to_owned();
        let file_path = file_path.map(|s| s.to_owned());
        let ast_node_id = ast_node_id.map(|s| s.to_owned());
        let relation = relation.to_owned();
        self.executor.run(move |db| db.kb_add_ast_link(&kb_id, &symbol_name, file_path.as_deref(), ast_node_id.as_deref(), &relation, confidence)).await
    }

    async fn kb_get_memories_for_symbol(&self, symbol_name: &str) -> DbResult<Vec<KnowledgeEntry>> {
        let symbol_name = symbol_name.to_owned();
        self.executor.run(move |db| db.kb_get_memories_for_symbol(&symbol_name)).await
    }

    async fn kb_get_memories_for_file(&self, file_path: &str) -> DbResult<Vec<(String, KnowledgeEntry)>> {
        let file_path = file_path.to_owned();
        self.executor.run(move |db| db.kb_get_memories_for_file(&file_path)).await
    }

    async fn kb_delete_ast_links_for(&self, kb_id: &str) -> DbResult<()> {
        let kb_id = kb_id.to_owned();
        self.executor.run(move |db| db.kb_delete_ast_links_for(&kb_id)).await
    }

    async fn kb_ast_lazy_resolve(&self, link_id: i64, symbol_name: &str) -> DbResult<Option<String>> {
        let symbol_name = symbol_name.to_owned();
        self.executor.run(move |db| db.kb_ast_lazy_resolve(link_id, &symbol_name)).await
    }
}
