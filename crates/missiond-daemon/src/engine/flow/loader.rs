//! Flow loader — reads FlowDefinition from YAML files.
//!
//! Search order (when `project_root` is supplied):
//!   1. explicit `flow_path` (absolute, or relative to `project_root`)
//!   2. `<project_root>/.missiond/generated/flows/<flow_id>.yaml`
//!   3. `$MISSIOND_HOME/flows/<flow_id>.yaml`
//!
//! Aligns with `.missiond/v2/intent-flow.lisp :: F-methodology-to-executable-compile :: s5/s6`
//! and `.missiond/v2/intent-tools.lisp :: mission_workflow compile_methodology /
//! run_methodology`. The methodology compiler persists generated flows under
//! `<project_root>/.missiond/generated/flows`; this loader makes those flows
//! discoverable via `mission_flow_run` without a Lisp/intent change.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::Serialize;
use tracing::info;

use super::FlowDefinition;

/// Project-local generated flows dir, relative to `project_root`. Mirrors
/// `GENERATED_FLOWS_DIR` in `handlers::knowledge::workflow` so the writer
/// (compile_methodology persist) and reader (this loader) stay in lockstep.
pub const GENERATED_FLOWS_REL: &str = ".missiond/generated/flows";

/// Where a flow YAML was discovered. Surfaced to the MCP response so the
/// caller can tell core vs project-generated vs explicit-path apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowSource {
    ExplicitPath,
    ProjectGenerated,
    MissionHomeCore,
}

impl FlowSource {
    pub fn as_str(self) -> &'static str {
        match self {
            FlowSource::ExplicitPath => "explicit_path",
            FlowSource::ProjectGenerated => "project_generated",
            FlowSource::MissionHomeCore => "mission_home_core",
        }
    }
}

/// A loaded flow plus the path/source it came from.
#[derive(Debug, Clone)]
pub struct LoadedFlow {
    pub definition: FlowDefinition,
    pub path: PathBuf,
    pub source: FlowSource,
}

/// Discovered flow without parsing — used by list responses.
#[derive(Debug, Clone, Serialize)]
pub struct FlowEntry {
    pub id: String,
    pub path: PathBuf,
    pub source: FlowSource,
}

/// Merged listing of core + project-generated flows. `merged_ids` is
/// deduplicated with project-generated taking precedence over core (i.e. a
/// project-local override of a core flow id wins on the merged list).
#[derive(Debug, Clone, Serialize, Default)]
pub struct FlowList {
    pub core: Vec<FlowEntry>,
    pub generated: Vec<FlowEntry>,
    pub merged_ids: Vec<String>,
    pub searched_paths: Vec<PathBuf>,
}

/// Search a flow yaml by id across (project-generated → core), returning the
/// first hit. Errors include every path searched so the MCP response can be
/// honest about what was looked at.
pub fn load_flow_with_project(
    flow_id: &str,
    project_root: Option<&Path>,
) -> Result<LoadedFlow> {
    let mut searched: Vec<PathBuf> = Vec::new();

    if let Some(root) = project_root {
        let p = generated_flow_path(root, flow_id);
        if p.exists() {
            return read_flow(&p, FlowSource::ProjectGenerated);
        }
        searched.push(p);
    }

    let core = core_flow_path(flow_id);
    if core.exists() {
        return read_flow(&core, FlowSource::MissionHomeCore);
    }
    searched.push(core);

    Err(anyhow!(
        "Flow '{}' not found. Searched: {}",
        flow_id,
        format_searched(&searched)
    ))
}

/// Load a flow YAML directly from an explicit path. Caller is responsible
/// for choosing the path; this is the only entry that bypasses the search
/// order. The returned `definition.id` is what was inside the YAML, not the
/// caller-supplied path stem.
pub fn load_flow_from_path(path: &Path) -> Result<LoadedFlow> {
    if !path.exists() {
        return Err(anyhow!("Flow YAML not found at {}", path.display()));
    }
    read_flow(path, FlowSource::ExplicitPath)
}

/// List all available flow definitions across (project-generated, core).
/// `merged_ids` deduplicates with generated taking precedence over core.
pub fn list_flows_with_project(project_root: Option<&Path>) -> Result<FlowList> {
    let mut out = FlowList::default();

    if let Some(root) = project_root {
        let dir = root.join(GENERATED_FLOWS_REL);
        out.searched_paths.push(dir.clone());
        if dir.exists() {
            out.generated = read_dir_entries(&dir, FlowSource::ProjectGenerated)?;
        }
    }

    let core_dir = missiond_core::default_mission_home().join("flows");
    out.searched_paths.push(core_dir.clone());
    if core_dir.exists() {
        out.core = read_dir_entries(&core_dir, FlowSource::MissionHomeCore)?;
    }

    let mut merged: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for e in out.generated.iter().chain(out.core.iter()) {
        if seen.insert(e.id.clone()) {
            merged.push(e.id.clone());
        }
    }
    out.merged_ids = merged;
    Ok(out)
}

// ───────────────────────────────────────────────────────────────────────
// Backward-compatible shims — used by callers that have no project root
// signal (e.g. capability_usage.rs registered_flows). Behavior unchanged.
// ───────────────────────────────────────────────────────────────────────

/// Load a flow by id from `$MISSIOND_HOME/flows/{flow_id}.yaml`. Preserved
/// for callers without project context. New code should prefer
/// [`load_flow_with_project`].
#[allow(dead_code)]
pub fn load_flow(flow_id: &str) -> Result<FlowDefinition> {
    load_flow_with_project(flow_id, None).map(|lf| lf.definition)
}

/// List core-flow ids in `$MISSIOND_HOME/flows`. Preserved for callers
/// without project context. New code should prefer
/// [`list_flows_with_project`].
pub fn list_flows() -> Result<Vec<String>> {
    let core_dir = missiond_core::default_mission_home().join("flows");
    if !core_dir.exists() {
        return Ok(vec![]);
    }
    let entries = read_dir_entries(&core_dir, FlowSource::MissionHomeCore)?;
    Ok(entries.into_iter().map(|e| e.id).collect())
}

// ───────────────────────────────────────────────────────────────────────
// internals
// ───────────────────────────────────────────────────────────────────────

fn generated_flow_path(project_root: &Path, flow_id: &str) -> PathBuf {
    project_root
        .join(GENERATED_FLOWS_REL)
        .join(format!("{}.yaml", flow_id))
}

fn core_flow_path(flow_id: &str) -> PathBuf {
    missiond_core::default_mission_home()
        .join("flows")
        .join(format!("{}.yaml", flow_id))
}

fn read_flow(path: &Path, source: FlowSource) -> Result<LoadedFlow> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("read {}: {}", path.display(), e))?;
    let definition: FlowDefinition = serde_yaml::from_str(&content)
        .map_err(|e| anyhow!("parse {}: {}", path.display(), e))?;
    info!(
        flow_id = definition.id,
        nodes = definition.nodes.len(),
        source = source.as_str(),
        path = %path.display(),
        "Flow loaded"
    );
    Ok(LoadedFlow {
        definition,
        path: path.to_path_buf(),
        source,
    })
}

fn read_dir_entries(dir: &Path, source: FlowSource) -> Result<Vec<FlowEntry>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let id = if let Some(stripped) = name.strip_suffix(".yaml") {
            stripped.to_string()
        } else if let Some(stripped) = name.strip_suffix(".yml") {
            stripped.to_string()
        } else {
            continue;
        };
        out.push(FlowEntry {
            id,
            path,
            source,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn format_searched(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::flow::{FlowDefinition, GatherStrategy, NodeType};

    const HELLO_PARALLEL_YAML: &str = include_str!("examples/hello_parallel.yaml");

    #[test]
    fn parses_hello_parallel_example() {
        let def: FlowDefinition = serde_yaml::from_str(HELLO_PARALLEL_YAML)
            .expect("hello_parallel.yaml must parse into FlowDefinition");

        assert_eq!(def.id, "hello-parallel");
        assert_eq!(def.nodes.len(), 1);

        let node = &def.nodes[0];
        assert_eq!(node.id, "scatter");
        assert_eq!(node.save_as.as_deref(), Some("scatter_results"));

        match &node.node_type {
            NodeType::ParallelSlotTasks {
                parallelism,
                tasks,
                gather,
                timeout_secs,
            } => {
                assert_eq!(*parallelism, 3);
                assert_eq!(tasks.len(), 3);
                assert_eq!(tasks[0].id, "task_a");
                assert_eq!(tasks[1].id, "task_b");
                assert_eq!(tasks[2].id, "task_c");
                assert!(matches!(gather, GatherStrategy::Aggregate));
                assert_eq!(*timeout_secs, 60);
            }
            other => panic!("expected ParallelSlotTasks, got {:?}", other),
        }
    }

    fn write_yaml(dir: &Path, flow_id: &str, name: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("{}.yaml", flow_id));
        let yaml = format!(
            "id: {flow_id}\nname: \"{name}\"\nnodes:\n  - id: only\n    type: slot_task\n    model: opus\n    prompt: \"hi\"\n",
            flow_id = flow_id,
            name = name,
        );
        std::fs::write(&path, yaml).unwrap();
        path
    }

    #[test]
    fn load_flow_from_path_reads_explicit_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_yaml(tmp.path(), "explicit-flow", "explicit");
        let lf = load_flow_from_path(&path).unwrap();
        assert_eq!(lf.definition.id, "explicit-flow");
        assert_eq!(lf.path, path);
        assert_eq!(lf.source, FlowSource::ExplicitPath);
    }

    #[test]
    fn load_flow_from_path_errors_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope.yaml");
        let err = load_flow_from_path(&missing).unwrap_err().to_string();
        assert!(err.contains("not found"), "err = {}", err);
    }

    #[test]
    fn load_flow_with_project_finds_generated() {
        let tmp = tempfile::tempdir().unwrap();
        let gen_dir = tmp.path().join(GENERATED_FLOWS_REL);
        let path = write_yaml(&gen_dir, "methodology-foo", "Foo");

        let lf = load_flow_with_project("methodology-foo", Some(tmp.path())).unwrap();
        assert_eq!(lf.definition.id, "methodology-foo");
        assert_eq!(lf.path, path);
        assert_eq!(lf.source, FlowSource::ProjectGenerated);
    }

    #[test]
    fn load_flow_with_project_missing_lists_searched_paths() {
        let tmp = tempfile::tempdir().unwrap();
        // No file written. Both searched paths should be reported.
        let err = load_flow_with_project("ghost", Some(tmp.path()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("ghost"), "err = {}", err);
        let expected_gen = tmp
            .path()
            .join(GENERATED_FLOWS_REL)
            .join("ghost.yaml");
        assert!(
            err.contains(&expected_gen.display().to_string()),
            "missing generated path in err: {}",
            err
        );
        assert!(
            err.contains("flows"),
            "missing core flows path in err: {}",
            err
        );
    }

    #[test]
    fn list_flows_with_project_merges_unique_generated_first() {
        let tmp = tempfile::tempdir().unwrap();
        let gen_dir = tmp.path().join(GENERATED_FLOWS_REL);
        write_yaml(&gen_dir, "alpha", "A");
        write_yaml(&gen_dir, "beta", "B");

        let list = list_flows_with_project(Some(tmp.path())).unwrap();
        assert!(list.merged_ids.contains(&"alpha".to_string()));
        assert!(list.merged_ids.contains(&"beta".to_string()));
        assert_eq!(list.generated.len(), 2);
        let dedup: std::collections::HashSet<_> = list.merged_ids.iter().collect();
        assert_eq!(
            dedup.len(),
            list.merged_ids.len(),
            "merged_ids should be deduplicated"
        );
        assert!(list
            .searched_paths
            .iter()
            .any(|p| p.ends_with(GENERATED_FLOWS_REL)));
    }

    #[test]
    fn list_flows_with_project_no_root_only_searches_core() {
        let list = list_flows_with_project(None).unwrap();
        assert!(list.generated.is_empty());
        // Core dir may or may not exist on the dev box; we only check that
        // we did not pretend to have searched a generated dir.
        assert!(!list
            .searched_paths
            .iter()
            .any(|p| p.ends_with(GENERATED_FLOWS_REL)));
    }

    #[test]
    fn load_flow_backward_compat_searches_only_core() {
        // The legacy `load_flow` should not pick up project-generated flows
        // (they require an explicit project_root). This guards against
        // accidentally widening behavior for callers like
        // capability_usage::registered_flows that pass no project context.
        let tmp = tempfile::tempdir().unwrap();
        let gen_dir = tmp.path().join(GENERATED_FLOWS_REL);
        write_yaml(&gen_dir, "isolated-only-in-project", "X");

        // Legacy call has no project signal — must miss the project-local
        // YAML and fall through to core, which definitely lacks this id.
        let res = load_flow("isolated-only-in-project");
        assert!(res.is_err(), "legacy load_flow leaked into project dir");
    }
}
