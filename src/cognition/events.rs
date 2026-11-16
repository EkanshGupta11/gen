use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CognitiveEventType {
    CycleStarted,
    StateUpdated,
    GoalCreated,
    GoalStatusChanged,
    MemoryItemInserted,
    MemoryItemUpdated,
    MemoryItemEvicted,
    AttentionFocused,
    ActionSelected,
    ActionPreconditionValidated,
    ActionPreconditionFailed,
    ActionExecuted,
    ActionEvaluation,
    MetacognitiveEvaluation,
    InvestigationStopped,
}

impl CognitiveEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CycleStarted => "cycle_started",
            Self::StateUpdated => "state_updated",
            Self::GoalCreated => "goal_created",
            Self::GoalStatusChanged => "goal_status_changed",
            Self::MemoryItemInserted => "memory_item_inserted",
            Self::MemoryItemUpdated => "memory_item_updated",
            Self::MemoryItemEvicted => "memory_item_evicted",
            Self::AttentionFocused => "attention_focused",
            Self::ActionSelected => "action_selected",
            Self::ActionPreconditionValidated => "action_precondition_validated",
            Self::ActionPreconditionFailed => "action_precondition_failed",
            Self::ActionExecuted => "action_executed",
            Self::ActionEvaluation => "action_evaluation",
            Self::MetacognitiveEvaluation => "metacognitive_evaluation",
            Self::InvestigationStopped => "investigation_stopped",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CognitiveEvent {
    pub id: String,
    pub investigation_id: String,
    pub cycle: u32,
    pub timestamp: String,
    pub event_type: CognitiveEventType,
    pub payload: Value,
}

impl CognitiveEvent {
    pub fn new(
        investigation_id: impl Into<String>,
        cycle: u32,
        event_type: CognitiveEventType,
        payload: Value,
    ) -> Self {
        let timestamp = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        Self {
            id: Uuid::new_v4().to_string(),
            investigation_id: investigation_id.into(),
            cycle,
            timestamp,
            event_type,
            payload,
        }
    }
}
