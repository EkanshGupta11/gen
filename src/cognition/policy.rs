use std::collections::VecDeque;

use crate::cognition::{
    actions::ExecutiveAction,
    goals::GoalStatus,
    state::{CognitiveState, ExecutivePhase},
};

pub const ALLOWED_SCIENTIFIC_TOOLS: &[&str] = &[
    "arithmetic",
    "projectile",
    "power_law",
    "polynomial",
    "statistics",
    "symbolic",
    "linear_algebra",
    "ode_solve",
];

pub fn validate_action_preconditions(
    action: &ExecutiveAction,
    state: &CognitiveState,
) -> Result<(), String> {
    if state.executive.current_phase == ExecutivePhase::Stopped {
        return Err("Cannot execute actions on a stopped cognitive controller".into());
    }

    match action {
        ExecutiveAction::ExecuteScientificTool {
            tool,
            arguments,
            goal_id,
        } => {
            if !ALLOWED_SCIENTIFIC_TOOLS.contains(&tool.as_str()) {
                return Err(format!(
                    "Tool '{tool}' is not in the allowed scientific tool allowlist: {:?}",
                    ALLOWED_SCIENTIFIC_TOOLS
                ));
            }
            if !arguments.is_object() {
                return Err(format!("Arguments for tool '{tool}' must be a JSON object"));
            }
            if !state.goals.goals.contains_key(goal_id) {
                return Err(format!(
                    "Associated goal '{goal_id}' does not exist in cognitive state"
                ));
            }
            let goal = &state.goals.goals[goal_id];
            if goal.status != GoalStatus::Active && goal.status != GoalStatus::Pending {
                return Err(format!(
                    "Associated goal '{goal_id}' is {:?}, not active",
                    goal.status
                ));
            }
        }
        ExecutiveAction::RequestSpecialistAnalysis {
            specialist,
            goal_id,
            ..
        } => {
            if !state.goals.goals.contains_key(goal_id) {
                return Err(format!(
                    "Associated goal '{goal_id}' does not exist in cognitive state"
                ));
            }
            let goal = &state.goals.goals[goal_id];
            if goal.status.is_terminal() {
                return Err(format!(
                    "Cannot request analysis for terminal goal '{goal_id}' ({:?})",
                    goal.status
                ));
            }
            let _ = specialist.allowed_tools();
        }
        ExecutiveAction::VerifyCalculation { tool_id, goal_id } => {
            if !state.goals.goals.contains_key(goal_id) {
                return Err(format!(
                    "Associated goal '{goal_id}' does not exist in cognitive state"
                ));
            }
            if tool_id.trim().is_empty() {
                return Err("Tool ID to verify cannot be empty".into());
            }
        }
        ExecutiveAction::DecomposeGoal {
            parent_goal_id,
            subgoals,
        } => {
            let parent = state
                .goals
                .goals
                .get(parent_goal_id)
                .ok_or_else(|| format!("Parent goal '{parent_goal_id}' does not exist"))?;
            if parent.status.is_terminal() {
                return Err(format!(
                    "Cannot decompose terminal parent goal '{parent_goal_id}' ({:?})",
                    parent.status
                ));
            }
            if subgoals.is_empty() {
                return Err("Decomposition must provide at least one subgoal".into());
            }
            for (idx, sub) in subgoals.iter().enumerate() {
                if sub.description.trim().is_empty() {
                    return Err(format!("Subgoal #{idx} has empty description"));
                }
            }
        }
        ExecutiveAction::ResolveContradiction {
            contradiction_id, ..
        } => {
            let contradiction = state
                .contradictions
                .iter()
                .find(|c| &c.id == contradiction_id)
                .ok_or_else(|| {
                    format!("Contradiction '{contradiction_id}' not found in cognitive state")
                })?;
            if contradiction.resolved {
                return Err(format!(
                    "Contradiction '{contradiction_id}' is already resolved"
                ));
            }
        }
        ExecutiveAction::SynthesizeFindings { goal_ids } => {
            if goal_ids.is_empty() && state.goals.goals.is_empty() {
                return Err("Cannot synthesize findings: no goals exist in cognitive state".into());
            }
        }
        ExecutiveAction::CompareHypotheses { hypothesis_ids } => {
            if hypothesis_ids.len() < 2 {
                return Err("Comparing hypotheses requires at least 2 hypothesis IDs".into());
            }
            for id in hypothesis_ids {
                if !state.hypotheses.contains_key(id) {
                    return Err(format!("Hypothesis '{id}' not found in cognitive state"));
                }
            }
        }
        ExecutiveAction::ClarifyMissingInformation {
            target_variable, ..
        } => {
            if target_variable.trim().is_empty() {
                return Err("Clarification target variable cannot be empty".into());
            }
        }
        ExecutiveAction::RetrieveKnowledge { query, .. } => {
            if query.trim().is_empty() {
                return Err("Knowledge retrieval query cannot be empty".into());
            }
        }
        ExecutiveAction::UpdateCognitiveState { update_summary } => {
            if update_summary.trim().is_empty() {
                return Err("State update summary cannot be empty".into());
            }
        }
        ExecutiveAction::RequestHumanInput { prompt } => {
            if prompt.trim().is_empty() {
                return Err("Human input request prompt cannot be empty".into());
            }
        }
        ExecutiveAction::StopInvestigation { .. } => {}
    }

    Ok(())
}

#[derive(Clone, Debug)]
pub struct LoopDetector {
    recent_action_signatures: VecDeque<String>,
    max_history: usize,
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self::new(6)
    }
}

impl LoopDetector {
    pub fn new(max_history: usize) -> Self {
        Self {
            recent_action_signatures: VecDeque::with_capacity(max_history),
            max_history,
        }
    }

    pub fn record_and_check_loop(&mut self, action: &ExecutiveAction) -> bool {
        let signature = format!("{}:{:?}", action.name(), action.associated_goal_id());
        self.recent_action_signatures.push_back(signature.clone());
        if self.recent_action_signatures.len() > self.max_history {
            self.recent_action_signatures.pop_front();
        }

        // Loop detection rule: if the same action signature appears 3 or more times in the last 4 actions
        if self.recent_action_signatures.len() >= 4 {
            let count = self
                .recent_action_signatures
                .iter()
                .rev()
                .take(4)
                .filter(|s| *s == &signature)
                .count();
            if count >= 3 {
                return true;
            }
        }
        false
    }

    pub fn clear(&mut self) {
        self.recent_action_signatures.clear();
    }
}
