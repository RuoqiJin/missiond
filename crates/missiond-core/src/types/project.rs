use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    pub path: String,
    pub intent_path: Option<String>,
    pub active: bool,
    pub slots: Vec<String>,
    pub github_url: Option<String>,
    /// "managed" (self-owned, direct filesystem) or "reference" (third-party, vault-cached)
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Vault cache directory for reference projects (e.g. ~/.missiond/vault/{name}/)
    pub vault_path: Option<String>,
    /// Monorepo parent project ID (sub-services point to parent)
    pub parent_id: Option<String>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn default_kind() -> String {
    "managed".to_string()
}

#[derive(Debug, Clone, Default)]
pub struct ProjectRegistry {
    projects: Vec<ProjectConfig>,
    path_index: Vec<(String, String)>,
}

impl ProjectRegistry {
    pub fn new(projects: Vec<ProjectConfig>) -> Self {
        let mut path_index: Vec<(String, String)> = projects
            .iter()
            .map(|p| (p.path.clone(), p.id.clone()))
            .collect();
        path_index.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        Self { projects, path_index }
    }

    pub fn resolve(&self, cwd: &str) -> Option<&str> {
        for (prefix, id) in &self.path_index {
            if cwd.starts_with(prefix.as_str()) {
                return Some(id.as_str());
            }
        }
        None
    }

    pub fn get(&self, id: &str) -> Option<&ProjectConfig> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn active_projects(&self) -> Vec<&ProjectConfig> {
        self.projects.iter().filter(|p| p.active).collect()
    }

    pub fn all_projects(&self) -> &[ProjectConfig] {
        &self.projects
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.projects.iter().any(|p| p.id == id && p.active)
    }

    pub fn exclusive_slots(&self, project_id: &str) -> Vec<String> {
        let target = match self.get(project_id) {
            Some(p) => p,
            None => return vec![],
        };
        let other_active_slots: HashSet<&str> = self.projects.iter()
            .filter(|p| p.id != project_id && p.active)
            .flat_map(|p| p.slots.iter().map(|s| s.as_str()))
            .collect();
        target.slots.iter()
            .filter(|s| !other_active_slots.contains(s.as_str()))
            .cloned()
            .collect()
    }
}

pub type SharedProjectRegistry = std::sync::Arc<tokio::sync::RwLock<ProjectRegistry>>;
