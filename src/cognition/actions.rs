use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contracts::Specialist;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    ObjectiveAchieved,
    AllGoalsCompleted,
    BudgetExhausted,
    CycleLimitReached,
    TimeoutExceeded,
    DeadlockNoExecutableActions,
    MetacognitiveAbort { rationale: String },
    UserCancelled,
}

impl StopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ObjectiveAchieved => "objective_achieved",
            Self::AllGoalsCompleted => "all_goals_completed",
            Self::BudgetExhausted => "budget_exhausted",
            Self::CycleLimitReached => "cycle_limit_reached",
            Self::TimeoutExceeded => "timeout_exceeded",
            Self::DeadlockNoExecutableActions => "deadlock_no_executable_actions",
            Self::MetacognitiveAbort { .. } => "metacognitive_abort",
            Self::UserCancelled => "user_cancelled",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SubgoalDef {
    pub description: String,
    pub priority: u32,
    pub dependencies: Vec<String>,
    pub required_tools: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ExecutiveAction {
    ClarifyMissingInformation {
        target_variable: String,
        reason: String,
    },
    RetrieveKnowledge {
        query: String,
        domain: String,
    },
    DecomposeGoal {
        parent_goal_id: String,
        subgoals: Vec<SubgoalDef>,
    },
    RequestSpecialistAnalysis {
        specialist: Specialist,
        task: String,
        goal_id: String,
    },
    ExecuteScientificTool {
        tool: String,
        arguments: Value,
        goal_id: String,
    },
    VerifyCalculation {
        tool_id: String,
        goal_id: String,
    },
    CompareHypotheses {
        hypothesis_ids: Vec<String>,
    },
    UpdateCognitiveState {
        update_summary: String,
    },
    ResolveContradiction {
        contradiction_id: String,
        resolution_strategy: String,
    },
    SynthesizeFindings {
        goal_ids: Vec<String>,
    },
    RequestHumanInput {
        prompt: String,
    },
    StopInvestigation {
        reason: StopReason,
    },
}

impl ExecutiveAction {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ClarifyMissingInformation { .. } => "clarify_missing_information",
            Self::RetrieveKnowledge { .. } => "retrieve_knowledge",
            Self::DecomposeGoal { .. } => "decompose_goal",
            Self::RequestSpecialistAnalysis { .. } => "request_specialist_analysis",
            Self::ExecuteScientificTool { .. } => "execute_scientific_tool",
            Self::VerifyCalculation { .. } => "verify_calculation",
            Self::CompareHypotheses { .. } => "compare_hypotheses",
            Self::UpdateCognitiveState { .. } => "update_cognitive_state",
            Self::ResolveContradiction { .. } => "resolve_contradiction",
            Self::SynthesizeFindings { .. } => "synthesize_findings",
            Self::RequestHumanInput { .. } => "request_human_input",
            Self::StopInvestigation { .. } => "stop_investigation",
        }
    }

    pub fn associated_goal_id(&self) -> Option<&str> {
        match self {
            Self::RequestSpecialistAnalysis { goal_id, .. } => Some(goal_id.as_str()),
            Self::ExecuteScientificTool { goal_id, .. } => Some(goal_id.as_str()),
            Self::VerifyCalculation { goal_id, .. } => Some(goal_id.as_str()),
            Self::DecomposeGoal { parent_goal_id, .. } => Some(parent_goal_id.as_str()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionExecutionStatus {
    Succeeded,
    Failed,
    Contradicted,
    Uncertain,
    Blocked,
    Unsupported,
    AwaitingHumanInput,
    Cancelled,
}

impl ActionExecutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Contradicted => "contradicted",
            Self::Uncertain => "uncertain",
            Self::Blocked => "blocked",
            Self::Unsupported => "unsupported",
            Self::AwaitingHumanInput => "awaiting_human_input",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Succeeded)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActionResult {
    pub action_id: String,
    pub action_name: String,
    pub status: ActionExecutionStatus,
    pub success: bool,
    pub output: Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

impl ActionResult {
    pub fn new(
        action_id: impl Into<String>,
        action_name: impl Into<String>,
        status: ActionExecutionStatus,
        output: Value,
        error: Option<String>,
        duration_ms: u64,
    ) -> Self {
        let is_success = status.is_success();
        Self {
            action_id: action_id.into(),
            action_name: action_name.into(),
            status,
            success: is_success,
            output,
            error,
            duration_ms,
        }
    }

    pub fn succeeded(
        action_id: impl Into<String>,
        action_name: impl Into<String>,
        output: Value,
        duration_ms: u64,
    ) -> Self {
        Self::new(
            action_id,
            action_name,
            ActionExecutionStatus::Succeeded,
            output,
            None,
            duration_ms,
        )
    }

    pub fn failed(
        action_id: impl Into<String>,
        action_name: impl Into<String>,
        error: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self::new(
            action_id,
            action_name,
            ActionExecutionStatus::Failed,
            Value::Null,
            Some(error.into()),
            duration_ms,
        )
    }

    pub fn unsupported(
        action_id: impl Into<String>,
        action_name: impl Into<String>,
        reason: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self::new(
            action_id,
            action_name,
            ActionExecutionStatus::Unsupported,
            Value::Null,
            Some(reason.into()),
            duration_ms,
        )
    }

    pub fn contradicted(
        action_id: impl Into<String>,
        action_name: impl Into<String>,
        output: Value,
        reason: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self::new(
            action_id,
            action_name,
            ActionExecutionStatus::Contradicted,
            output,
            Some(reason.into()),
            duration_ms,
        )
    }
}
