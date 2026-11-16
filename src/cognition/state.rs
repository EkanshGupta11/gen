use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    cognition::{
        actions::{ActionResult, ExecutiveAction, StopReason},
        goals::GoalManager,
        working_memory::{KnowledgeItemType, WorkingMemory},
    },
    contracts::EpistemicStatus,
};

pub const COGNITIVE_STATE_SCHEMA_VERSION: &str = "1.0";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutivePhase {
    Idle,
    Observe,
    UpdateState,
    EvaluateGoals,
    AttentionPrioritisation,
    SelectAction,
    ValidatePreconditions,
    ExecuteAction,
    EvaluateOutcome,
    UpdateWorkingMemory,
    MetacognitiveEvaluation,
    Stopped,
}

impl ExecutivePhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Observe => "observe",
            Self::UpdateState => "update_state",
            Self::EvaluateGoals => "evaluate_goals",
            Self::AttentionPrioritisation => "attention_prioritisation",
            Self::SelectAction => "select_action",
            Self::ValidatePreconditions => "validate_preconditions",
            Self::ExecuteAction => "execute_action",
            Self::EvaluateOutcome => "evaluate_outcome",
            Self::UpdateWorkingMemory => "update_working_memory",
            Self::MetacognitiveEvaluation => "metacognitive_evaluation",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InvestigationContext {
    pub investigation_id: String,
    pub primary_objective: String,
    pub constraints: Vec<String>,
    pub relevant_entities: Vec<String>,
    pub known_variables: Vec<String>,
    pub unknown_variables: Vec<String>,
    pub current_stage: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Provenance {
    pub source_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    pub timestamp: String,
}

impl Provenance {
    pub fn new(source_type: impl Into<String>) -> Self {
        let timestamp = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        Self {
            source_type: source_type.into(),
            source_id: None,
            generated_by: None,
            timestamp,
        }
    }

    pub fn with_source_id(mut self, id: impl Into<String>) -> Self {
        self.source_id = Some(id.into());
        self
    }

    pub fn with_generated_by(mut self, generator: impl Into<String>) -> Self {
        self.generated_by = Some(generator.into());
        self
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Hypothesis {
    pub id: String,
    pub statement: String,
    pub supporting_observations: Vec<String>,
    pub contradicting_observations: Vec<String>,
    pub assumptions: Vec<String>,
    pub epistemic_status: EpistemicStatus,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KnowledgeItem {
    pub id: String,
    pub item_type: KnowledgeItemType,
    pub content: Value,
    pub provenance: Provenance,
    pub epistemic_status: EpistemicStatus,
    pub related_goals: Vec<String>,
    pub related_entities: Vec<String>,
    pub timestamp: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionOutcome {
    Resolved,
    PartiallyResolved,
    Unresolved,
    RequiresHumanReview,
}

impl ResolutionOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::PartiallyResolved => "partially_resolved",
            Self::Unresolved => "unresolved",
            Self::RequiresHumanReview => "requires_human_review",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContradictionResolution {
    pub contradiction_id: String,
    pub conflicting_items: Vec<String>,
    pub proposed_resolution: String,
    pub supporting_evidence_refs: Vec<String>,
    pub verification_method: String,
    pub outcome: ResolutionOutcome,
    pub timestamp: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContradictionRecord {
    pub id: String,
    pub description: String,
    pub conflicting_items: Vec<String>,
    pub resolved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<ContradictionResolution>,
    pub detected_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExecutiveControllerState {
    pub current_cycle: u32,
    pub current_phase: ExecutivePhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_action: Option<ExecutiveAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_action_outcome: Option<ActionResult>,
    #[serde(default)]
    pub total_actions_executed: u32,
    pub remaining_action_budget: u32,
    #[serde(default)]
    pub loop_detection_history: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CognitiveState {
    pub schema_version: String,
    pub context: InvestigationContext,
    pub goals: GoalManager,
    pub hypotheses: HashMap<String, Hypothesis>,
    pub knowledge_items: HashMap<String, KnowledgeItem>,
    pub working_memory: WorkingMemory,
    pub executive: ExecutiveControllerState,
    pub contradictions: Vec<ContradictionRecord>,
    pub consecutive_failures: u32,
}

impl CognitiveState {
    pub fn new(
        investigation_id: impl Into<String>,
        primary_objective: impl Into<String>,
        working_memory_capacity: usize,
        action_budget: u32,
    ) -> Self {
        Self {
            schema_version: COGNITIVE_STATE_SCHEMA_VERSION.into(),
            context: InvestigationContext {
                investigation_id: investigation_id.into(),
                primary_objective: primary_objective.into(),
                constraints: Vec::new(),
                relevant_entities: Vec::new(),
                known_variables: Vec::new(),
                unknown_variables: Vec::new(),
                current_stage: "initialization".into(),
            },
            goals: GoalManager::new(),
            hypotheses: HashMap::new(),
            knowledge_items: HashMap::new(),
            working_memory: WorkingMemory::new(working_memory_capacity),
            executive: ExecutiveControllerState {
                current_cycle: 0,
                current_phase: ExecutivePhase::Idle,
                selected_action: None,
                previous_action_outcome: None,
                total_actions_executed: 0,
                remaining_action_budget: action_budget,
                loop_detection_history: Vec::new(),
                stop_reason: None,
            },
            contradictions: Vec::new(),
            consecutive_failures: 0,
        }
    }

    pub fn add_knowledge_item(&mut self, item: KnowledgeItem) {
        let wm_item = crate::cognition::working_memory::WorkingMemoryItem::new(
            item.id.clone(),
            item.item_type.clone(),
            item.content.clone(),
            item.epistemic_status.clone(),
        )
        .with_related_goals(item.related_goals.clone());

        let _ = self.working_memory.insert(wm_item);
        self.knowledge_items.insert(item.id.clone(), item);
    }

    pub fn record_contradiction(
        &mut self,
        id: impl Into<String>,
        description: impl Into<String>,
        conflicting_items: Vec<String>,
    ) {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let contradiction_id = id.into();
        self.contradictions.push(ContradictionRecord {
            id: contradiction_id.clone(),
            description: description.into(),
            conflicting_items,
            resolved: false,
            resolution: None,
            detected_at: now,
        });
    }

    pub fn unresolved_contradictions(&self) -> Vec<&ContradictionRecord> {
        self.contradictions.iter().filter(|c| !c.resolved).collect()
    }

    pub fn is_stopped(&self) -> bool {
        self.executive.stop_reason.is_some()
            || self.executive.current_phase == ExecutivePhase::Stopped
    }
}

/// Checks whether a verified knowledge item is scientifically comparable to another conflicting item.
/// Returns Ok(()) if the items share matching tools/topics, compatible conditions/arguments,
/// compatible units, and non-conflicting physical/model assumptions.
pub fn check_contradiction_comparability(
    verified: &KnowledgeItem,
    other: &KnowledgeItem,
) -> Result<(), String> {
    // 1. Tool check: If both specify a tool, ensure they match
    let tool_v = verified.content.get("tool").and_then(Value::as_str);
    let tool_o = other.content.get("tool").and_then(Value::as_str);
    if let (Some(tv), Some(to)) = (tool_v, tool_o)
        && tv != to
    {
        return Err(format!("Different tools used: '{tv}' vs '{to}'"));
    }

    // 2. Condition / Parameter comparison
    let args_v = verified.content.get("arguments").and_then(Value::as_object);
    let args_o = other.content.get("arguments").and_then(Value::as_object);
    if let (Some(av), Some(ao)) = (args_v, args_o) {
        for (k, val_v) in av {
            if let Some(val_o) = ao.get(k) {
                if let (Some(nv), Some(no)) = (val_v.as_f64(), val_o.as_f64()) {
                    if (nv - no).abs() > 1e-4 {
                        return Err(format!(
                            "Condition parameter mismatch for '{k}': {nv} vs {no}"
                        ));
                    }
                } else if val_v != val_o {
                    return Err(format!(
                        "Condition parameter mismatch for '{k}': {:?} vs {:?}",
                        val_v, val_o
                    ));
                }
            }
        }
    }

    // 3. Units comparison
    let unit_v = verified
        .content
        .get("units")
        .or_else(|| verified.content.get("unit"));
    let unit_o = other
        .content
        .get("units")
        .or_else(|| other.content.get("unit"));
    if let (Some(uv), Some(uo)) = (unit_v, unit_o)
        && uv != uo
    {
        return Err(format!("Incompatible units: {:?} vs {:?}", uv, uo));
    }

    if let (Some(av), Some(ao)) = (args_v, args_o) {
        let u_v = av.get("units").or_else(|| av.get("unit"));
        let u_o = ao.get("units").or_else(|| ao.get("unit"));
        if let (Some(uv), Some(uo)) = (u_v, u_o)
            && uv != uo
        {
            return Err(format!(
                "Incompatible units in arguments: {:?} vs {:?}",
                uv, uo
            ));
        }
    }

    // 4. Assumptions comparison
    let assumptions_v = extract_assumptions(&verified.content);
    let assumptions_o = extract_assumptions(&other.content);

    let v_has_air = assumptions_v
        .iter()
        .any(|a| a.contains("air") || a.contains("drag") || a.contains("resistance"));
    let o_has_air = assumptions_o
        .iter()
        .any(|a| a.contains("air") || a.contains("drag") || a.contains("resistance"));
    let v_has_vacuum = assumptions_v
        .iter()
        .any(|a| a.contains("vacuum") || a.contains("ideal") || a.contains("no drag"));
    let o_has_vacuum = assumptions_o
        .iter()
        .any(|a| a.contains("vacuum") || a.contains("ideal") || a.contains("no drag"));

    if (v_has_air && o_has_vacuum) || (v_has_vacuum && o_has_air) {
        return Err("Conflicting physical assumptions: air resistance vs ideal vacuum".into());
    }

    for a_v in &assumptions_v {
        for a_o in &assumptions_o {
            if (a_v.contains("relativistic") && a_o.contains("classical"))
                || (a_v.contains("classical") && a_o.contains("relativistic"))
            {
                return Err(format!("Conflicting model assumptions: '{a_v}' vs '{a_o}'"));
            }
        }
    }

    Ok(())
}

fn extract_assumptions(val: &Value) -> Vec<String> {
    let mut assumptions = Vec::new();
    if let Some(arr) = val.get("assumptions").and_then(Value::as_array) {
        for item in arr {
            if let Some(s) = item.as_str() {
                assumptions.push(s.to_lowercase());
            }
        }
    } else if let Some(s) = val.get("assumptions").and_then(Value::as_str) {
        assumptions.push(s.to_lowercase());
    }
    assumptions
}
