//! Flow loader — reads FlowDefinition from YAML files in $MISSIOND_HOME/flows/.

use anyhow::{anyhow, Result};
use tracing::info;

use super::FlowDefinition;

/// Load a flow definition by ID from the flows directory.
/// Searches: $MISSIOND_HOME/flows/{flow_id}.yaml
pub fn load_flow(flow_id: &str) -> Result<FlowDefinition> {
    let flows_dir = missiond_core::default_mission_home().join("flows");
    let path = flows_dir.join(format!("{}.yaml", flow_id));
    if !path.exists() {
        return Err(anyhow!("Flow '{}' not found at {}", flow_id, path.display()));
    }
    let content = std::fs::read_to_string(&path)?;
    let def: FlowDefinition = serde_yaml::from_str(&content)?;
    info!(flow_id = def.id, nodes = def.nodes.len(), "Flow loaded");
    Ok(def)
}

/// List all available flow definitions.
pub fn list_flows() -> Result<Vec<String>> {
    let flows_dir = missiond_core::default_mission_home().join("flows");
    if !flows_dir.exists() {
        return Ok(vec![]);
    }
    let mut flows = Vec::new();
    for entry in std::fs::read_dir(&flows_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".yaml") || name.ends_with(".yml") {
            flows.push(name.trim_end_matches(".yaml").trim_end_matches(".yml").to_string());
        }
    }
    Ok(flows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::flow::{FlowDefinition, GatherStrategy, NodeType};

    const HELLO_PARALLEL_YAML: &str =
        include_str!("examples/hello_parallel.yaml");

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
}
