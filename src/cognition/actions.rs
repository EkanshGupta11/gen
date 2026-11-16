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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActionResult {
    pub action_id: String,
    pub action_name: String,
    pub success: bool,
    pub output: Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}
