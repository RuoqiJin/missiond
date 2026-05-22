use std::collections::BTreeSet;

use missiond_kernel::{
    ActivationMode, Genome, Molecule, OrganProfile, RuntimeBudgets, TissueProfile,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const GENOME_SCHEMA: &str = "missiond.genome.v1";

#[derive(Debug, Error)]
pub enum GenomeError {
    #[error("invalid genome json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Validation(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledGenomeEnvelope {
    pub schema_version: String,
    pub source_hash: Option<String>,
    pub generated_at: Option<String>,
    #[serde(default)]
    pub diagnostics: Vec<serde_json::Value>,
    pub payload: CompiledGenomePayload,
}

impl CompiledGenomeEnvelope {
    pub fn from_json_str(input: &str) -> Result<Self, GenomeError> {
        let envelope: Self = serde_json::from_str(input)?;
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), GenomeError> {
        if self.schema_version != "missiond.compiled-genomes.v1" {
            return Err(GenomeError::Validation(format!(
                "unsupported compiled genome schema: {}",
                self.schema_version
            )));
        }
        if self.payload.genomes.is_empty() {
            return Err(GenomeError::Validation(
                "compiled genome payload has no genomes".to_string(),
            ));
        }
        for genome in &self.payload.genomes {
            genome.validate()?;
        }
        Ok(())
    }

    pub fn to_kernel_genomes(&self) -> Result<Vec<Genome>, GenomeError> {
        self.validate()?;
        self.payload
            .genomes
            .iter()
            .map(CompiledGenome::to_kernel)
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledGenomePayload {
    #[serde(default)]
    pub genomes: Vec<CompiledGenome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledGenome {
    pub id: String,
    pub schema: String,
    pub activation: String,
    #[serde(default)]
    pub organs: Vec<CompiledOrgan>,
}

impl CompiledGenome {
    pub fn validate(&self) -> Result<(), GenomeError> {
        require_non_empty("genome id", &self.id)?;
        if self.schema != GENOME_SCHEMA {
            return Err(GenomeError::Validation(format!(
                "genome {} declares unsupported schema {}",
                self.id, self.schema
            )));
        }
        if ActivationMode::parse(&self.activation).is_none() {
            return Err(GenomeError::Validation(format!(
                "genome {} declares unsupported activation {}",
                self.id, self.activation
            )));
        }
        if self.organs.is_empty() {
            return Err(GenomeError::Validation(format!(
                "genome {} has no organs",
                self.id
            )));
        }
        for organ in &self.organs {
            organ.validate(&self.id)?;
        }
        Ok(())
    }

    pub fn to_kernel(&self) -> Result<Genome, GenomeError> {
        self.validate()?;
        Ok(Genome {
            id: self.id.clone(),
            schema: self.schema.clone(),
            activation: ActivationMode::parse(&self.activation).ok_or_else(|| {
                GenomeError::Validation(format!("invalid activation {}", self.activation))
            })?,
            organs: self
                .organs
                .iter()
                .map(CompiledOrgan::to_kernel)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledOrgan {
    pub id: String,
    #[serde(default)]
    pub tissues: Vec<CompiledTissue>,
}

impl CompiledOrgan {
    fn validate(&self, genome_id: &str) -> Result<(), GenomeError> {
        require_non_empty("organ id", &self.id)?;
        if self.tissues.is_empty() {
            return Err(GenomeError::Validation(format!(
                "genome {genome_id} organ {} has no tissues",
                self.id
            )));
        }
        for tissue in &self.tissues {
            tissue.validate(genome_id, &self.id)?;
        }
        Ok(())
    }

    fn to_kernel(&self) -> Result<OrganProfile, GenomeError> {
        Ok(OrganProfile {
            id: self.id.clone(),
            tissues: self
                .tissues
                .iter()
                .map(CompiledTissue::to_kernel)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledTissue {
    pub id: String,
    #[serde(default)]
    pub receptors: Vec<String>,
    #[serde(default)]
    pub allow_atoms: Vec<String>,
    #[serde(default)]
    pub allow_effects: Vec<String>,
    #[serde(default)]
    pub molecules: Vec<CompiledMolecule>,
    pub budgets: Option<RuntimeBudgets>,
}

impl CompiledTissue {
    fn validate(&self, genome_id: &str, organ_id: &str) -> Result<(), GenomeError> {
        require_non_empty("tissue id", &self.id)?;
        if self.receptors.is_empty() {
            return Err(GenomeError::Validation(format!(
                "{genome_id}/{organ_id}/{} has no receptors",
                self.id
            )));
        }
        if self.allow_effects.is_empty() {
            return Err(GenomeError::Validation(format!(
                "{genome_id}/{organ_id}/{} has no allowed effects",
                self.id
            )));
        }
        if self.molecules.is_empty() {
            return Err(GenomeError::Validation(format!(
                "{genome_id}/{organ_id}/{} has no molecules",
                self.id
            )));
        }
        let allowed_atoms = self.allow_atoms.iter().collect::<BTreeSet<_>>();
        let allowed_effects = self.allow_effects.iter().collect::<BTreeSet<_>>();
        for molecule in &self.molecules {
            molecule.validate(
                genome_id,
                organ_id,
                &self.id,
                &allowed_atoms,
                &allowed_effects,
            )?;
        }
        let budgets = self.budgets.clone().unwrap_or_default();
        if budgets.max_causation_depth == 0
            || budgets.max_events_per_correlation == 0
            || budgets.max_cell_runtime_ms == 0
            || budgets.idempotency_cache_size == 0
        {
            return Err(GenomeError::Validation(format!(
                "{genome_id}/{organ_id}/{} has invalid zero budget",
                self.id
            )));
        }
        Ok(())
    }

    fn to_kernel(&self) -> Result<TissueProfile, GenomeError> {
        Ok(TissueProfile {
            id: self.id.clone(),
            receptors: self.receptors.clone(),
            allow_atoms: self.allow_atoms.clone(),
            allow_effects: self.allow_effects.clone(),
            molecules: self
                .molecules
                .iter()
                .map(CompiledMolecule::to_kernel)
                .collect(),
            budgets: self.budgets.clone().unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledMolecule {
    pub id: String,
    pub on: String,
    pub when: Option<String>,
    #[serde(default)]
    pub effects: Vec<String>,
    #[serde(default)]
    pub atoms: Vec<String>,
}

impl CompiledMolecule {
    fn validate(
        &self,
        genome_id: &str,
        organ_id: &str,
        tissue_id: &str,
        allowed_atoms: &BTreeSet<&String>,
        allowed_effects: &BTreeSet<&String>,
    ) -> Result<(), GenomeError> {
        require_non_empty("molecule id", &self.id)?;
        require_non_empty("molecule on", &self.on)?;
        if self.effects.is_empty() {
            return Err(GenomeError::Validation(format!(
                "{genome_id}/{organ_id}/{tissue_id}/{} has no effects",
                self.id
            )));
        }
        for atom in &self.atoms {
            if !allowed_atoms.contains(atom) {
                return Err(GenomeError::Validation(format!(
                    "{genome_id}/{organ_id}/{tissue_id}/{} uses unallowed atom {}",
                    self.id, atom
                )));
            }
        }
        for effect in &self.effects {
            if !allowed_effects.contains(effect) {
                return Err(GenomeError::Validation(format!(
                    "{genome_id}/{organ_id}/{tissue_id}/{} uses unallowed effect {}",
                    self.id, effect
                )));
            }
        }
        Ok(())
    }

    fn to_kernel(&self) -> Molecule {
        Molecule {
            id: self.id.clone(),
            on: self.on.clone(),
            when: self.when.clone(),
            atoms: self.atoms.clone(),
            effects: self.effects.clone(),
            rule_graph: None,
        }
    }
}

fn require_non_empty(label: &str, value: &str) -> Result<(), GenomeError> {
    if value.trim().is_empty() {
        return Err(GenomeError::Validation(format!("{label} is empty")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_envelope() -> CompiledGenomeEnvelope {
        CompiledGenomeEnvelope {
            schema_version: "missiond.compiled-genomes.v1".to_string(),
            source_hash: Some("hash".to_string()),
            generated_at: None,
            diagnostics: Vec::new(),
            payload: CompiledGenomePayload {
                genomes: vec![CompiledGenome {
                    id: "missiond-autopilot".to_string(),
                    schema: GENOME_SCHEMA.to_string(),
                    activation: "shadow".to_string(),
                    organs: vec![CompiledOrgan {
                        id: "autopilot".to_string(),
                        tissues: vec![CompiledTissue {
                            id: "wakeup".to_string(),
                            receptors: vec!["BoardEvent::TaskCreated".to_string()],
                            allow_atoms: vec!["autopilot.match-board-wakeup".to_string()],
                            allow_effects: vec!["NotifyAutopilotDispatch".to_string()],
                            molecules: vec![CompiledMolecule {
                                id: "board-wakeup".to_string(),
                                on: "BoardEvent::TaskCreated".to_string(),
                                when: None,
                                effects: vec!["NotifyAutopilotDispatch".to_string()],
                                atoms: vec!["autopilot.match-board-wakeup".to_string()],
                            }],
                            budgets: Some(RuntimeBudgets::default()),
                        }],
                    }],
                }],
            },
        }
    }

    #[test]
    fn validates_compiled_genome() {
        let envelope = valid_envelope();
        envelope.validate().unwrap();
        let genomes = envelope.to_kernel_genomes().unwrap();
        assert_eq!(genomes[0].activation, ActivationMode::Shadow);
    }

    #[test]
    fn rejects_unallowed_effects() {
        let mut envelope = valid_envelope();
        envelope.payload.genomes[0].organs[0].tissues[0].molecules[0]
            .effects
            .push("RunBoardDispatch".to_string());
        let err = envelope.validate().unwrap_err();
        assert!(err.to_string().contains("unallowed effect"));
    }

    #[test]
    fn rejects_zero_budgets() {
        let mut envelope = valid_envelope();
        envelope.payload.genomes[0].organs[0].tissues[0]
            .budgets
            .as_mut()
            .unwrap()
            .max_cell_runtime_ms = 0;
        let err = envelope.validate().unwrap_err();
        assert!(err.to_string().contains("zero budget"));
    }
}
