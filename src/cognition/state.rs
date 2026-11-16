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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContradictionRecord {
    pub id: String,
    pub description: String,
    pub conflicting_items: Vec<String>,
    pub resolved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
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
    pub remaining_action_budget: u32,
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
                remaining_action_budget: action_budget,
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
