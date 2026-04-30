use super::*;

pub(in crate::handlers::knowledge::workflow) fn build_generated_yaml(
    meta: &GeneratedMeta,
    steps: &[LocatedStep],
    lifted: &MethodologyLifted,
    review_required: bool,
) -> Result<String, serde_yaml::Error> {
    use serde_yaml::{Mapping, Value as Yaml};

    let mut root = Mapping::new();
    root.insert(Yaml::from("id"), Yaml::from(meta.flow_id.clone()));
    root.insert(Yaml::from("name"), Yaml::from(meta.name.clone()));
    root.insert(Yaml::from("source_kind"), Yaml::from("methodology_lisp"));
    root.insert(
        Yaml::from("source_path"),
        Yaml::from(meta.source_path.clone()),
    );
    root.insert(
        Yaml::from("source_hash"),
        Yaml::from(meta.source_hash.clone()),
    );
    root.insert(Yaml::from("generated_by"), Yaml::from(COMPILER_VERSION));
    root.insert(
        Yaml::from("generated_at"),
        Yaml::from(meta.generated_at.clone()),
    );
    root.insert(
        Yaml::from("compiler_status"),
        Yaml::from(meta.compiler_status.clone()),
    );
    root.insert(Yaml::from("review_required"), Yaml::from(review_required));

    // Lifted higher-order semantics — emitted under a top-level
    // `methodology_metadata` mapping. `FlowDefinition` does NOT declare this
    // field, so serde_yaml ignores it during loader deserialisation while
    // the raw YAML still preserves it for human reviewers and the future
    // forge compiler. Keeping this strictly out-of-band is what lets the
    // v0 lifter stay conservative — no execution semantics change.
    if !lifted.is_empty() {
        root.insert(
            Yaml::from("methodology_metadata"),
            Yaml::Mapping(build_methodology_metadata_yaml(lifted)),
        );
    }

    let mut nodes_seq: Vec<Yaml> = Vec::new();
    if steps.is_empty() {
        let mut node = Mapping::new();
        node.insert(Yaml::from("id"), Yaml::from("manual_review"));
        node.insert(Yaml::from("type"), Yaml::from("slot_task"));
        node.insert(Yaml::from("model"), Yaml::from("opus"));
        node.insert(
            Yaml::from("prompt"),
            Yaml::from(build_manual_review_prompt(meta, lifted)),
        );
        // Mirror the lifted metadata onto the manual_review node itself so
        // the reviewer sees it without having to walk back to the YAML
        // root. The flattened FlowNode/NodeType serde shape ignores
        // unknown keys, so this is a pure documentation channel.
        if !lifted.is_empty() {
            node.insert(
                Yaml::from("methodology_metadata"),
                Yaml::Mapping(build_methodology_metadata_yaml(lifted)),
            );
        }
        nodes_seq.push(Yaml::Mapping(node));
    } else {
        for step in steps {
            let safe_id = sanitize_id_token(&step.step.id);
            let node_id = if safe_id.is_empty() {
                "step".to_string()
            } else {
                format!("step_{}", safe_id)
            };
            let mut node = Mapping::new();
            node.insert(Yaml::from("id"), Yaml::from(node_id.clone()));
            node.insert(Yaml::from("type"), Yaml::from("slot_task"));
            node.insert(Yaml::from("model"), Yaml::from("opus"));
            node.insert(Yaml::from("prompt"), Yaml::from(step.step.body.clone()));
            node.insert(
                Yaml::from("save_as"),
                Yaml::from(format!("{}_result", node_id)),
            );
            // Per-node `methodology_metadata.phase_id` carries the v0
            // lifter's phase association. FlowNode flattens NodeType (which
            // has `tag = "type"`); serde_yaml's default unknown-field
            // tolerance lets us attach this without affecting the
            // executable shape — verified by the YAML round-trip test.
            if let Some(phase_id) = phase_id_for_step(&lifted.phases, step.start_line) {
                let mut node_meta = Mapping::new();
                node_meta.insert(Yaml::from("phase_id"), Yaml::from(phase_id));
                node.insert(Yaml::from("methodology_metadata"), Yaml::Mapping(node_meta));
            }
            nodes_seq.push(Yaml::Mapping(node));
        }
    }
    root.insert(Yaml::from("nodes"), Yaml::Sequence(nodes_seq));
    serde_yaml::to_string(&Yaml::Mapping(root))
}

/// Build the prompt body for the `manual_review` fallback node. When the
/// v0 lifter recovered higher-order forms, surface them in the prompt so
/// the reviewer immediately sees what the methodology declared even before
/// touching the metadata mapping.
pub(in crate::handlers::knowledge::workflow) fn build_manual_review_prompt(
    meta: &GeneratedMeta,
    lifted: &MethodologyLifted,
) -> String {
    let base = format!(
        "Manually review compiled methodology '{flow}' before running.\n\
         Source: {src}\n\
         Source hash: {hash}\n\
         The deterministic compiler v0 could not auto-extract executable (step …) forms.\n\
         Edit this YAML or augment the source Lisp before dispatching.",
        flow = meta.flow_id,
        src = meta.source_path,
        hash = meta.source_hash,
    );
    if lifted.is_empty() {
        return base;
    }
    let mut out = base;
    out.push_str("\n\nLifted methodology semantics (v0 recognised, NOT executable):");
    if !lifted.phases.is_empty() {
        out.push_str(&format!("\n  - phases: {}", lifted.phases.len()));
    }
    if !lifted.principles.is_empty() {
        out.push_str(&format!("\n  - principles: {}", lifted.principles.len()));
    }
    if !lifted.anti_patterns.is_empty() {
        out.push_str(&format!(
            "\n  - anti-patterns: {}",
            lifted.anti_patterns.len()
        ));
    }
    if !lifted.gates.is_empty() {
        out.push_str(&format!("\n  - gates: {}", lifted.gates.len()));
    }
    if !lifted.artifacts.is_empty() {
        out.push_str(&format!("\n  - artifacts: {}", lifted.artifacts.len()));
    }
    if !lifted.authorities.is_empty() {
        out.push_str(&format!("\n  - authorities: {}", lifted.authorities.len()));
    }
    out.push_str("\nSee the `methodology_metadata` mapping at the YAML root for raw bodies.");
    out
}

/// Produce the YAML representation of the lifted methodology forms.
/// Each category is a sequence of `{kind, id?, body, start_line}` entries
/// (or `{id?, body, start_line, end_line}` for phases). Bodies are kept
/// verbatim so reviewers and the future forge compiler can recover the
/// exact source spelling.
fn build_methodology_metadata_yaml(lifted: &MethodologyLifted) -> serde_yaml::Mapping {
    use serde_yaml::{Mapping, Value as Yaml};

    fn form_to_yaml(form: &MethodologyForm) -> Yaml {
        let mut m = Mapping::new();
        m.insert(Yaml::from("kind"), Yaml::from(form.kind.clone()));
        if let Some(id) = &form.id {
            m.insert(Yaml::from("id"), Yaml::from(id.clone()));
        }
        m.insert(Yaml::from("body"), Yaml::from(form.body.clone()));
        m.insert(Yaml::from("start_line"), Yaml::from(form.start_line as u64));
        Yaml::Mapping(m)
    }

    let mut root = Mapping::new();
    if !lifted.phases.is_empty() {
        let phases_seq: Vec<Yaml> = lifted
            .phases
            .iter()
            .map(|ph| {
                let mut m = Mapping::new();
                m.insert(Yaml::from("kind"), Yaml::from("phase"));
                if let Some(id) = &ph.id {
                    m.insert(Yaml::from("id"), Yaml::from(id.clone()));
                }
                m.insert(Yaml::from("body"), Yaml::from(ph.body.clone()));
                m.insert(Yaml::from("start_line"), Yaml::from(ph.start_line as u64));
                m.insert(Yaml::from("end_line"), Yaml::from(ph.end_line as u64));
                Yaml::Mapping(m)
            })
            .collect();
        root.insert(Yaml::from("phases"), Yaml::Sequence(phases_seq));
    }
    if !lifted.principles.is_empty() {
        root.insert(
            Yaml::from("principles"),
            Yaml::Sequence(lifted.principles.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.anti_patterns.is_empty() {
        root.insert(
            Yaml::from("anti_patterns"),
            Yaml::Sequence(lifted.anti_patterns.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.gates.is_empty() {
        root.insert(
            Yaml::from("gates"),
            Yaml::Sequence(lifted.gates.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.artifacts.is_empty() {
        root.insert(
            Yaml::from("artifacts"),
            Yaml::Sequence(lifted.artifacts.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.authorities.is_empty() {
        root.insert(
            Yaml::from("authorities"),
            Yaml::Sequence(lifted.authorities.iter().map(form_to_yaml).collect()),
        );
    }
    root
}
