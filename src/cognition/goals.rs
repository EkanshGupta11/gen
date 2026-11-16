use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cognition::actions::ExecutiveAction;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Pending,
    Active,
    Completed,
    Blocked,
    Failed,
    Cancelled,
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Precondition {
    GoalCompleted(String),
    KnowledgeAvailable(String),
    ToolVerified(String),
    Custom(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CompletionCriterion {
    ToolExecuted { tool_name: String },
    CalculationVerified { subject_id: String },
    SpecialistReportReceived { specialist: String },
    SubGoalsCompleted,
    SynthesisCompleted,
    ObservationsRecorded { min_count: usize },
    HypothesesTested,
    ExplicitApproval { requirement: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Goal {
    pub id: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_goal: Option<String>,
    pub priority: u32,
    pub status: GoalStatus,
    pub dependencies: Vec<String>,
    pub preconditions: Vec<Precondition>,
    pub completion_criteria: Vec<CompletionCriterion>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgendaStatus {
    Ready,
    Blocked,
    Active,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgendaItem {
    pub id: String,
    pub goal_id: String,
    pub action: ExecutiveAction,
    pub status: AgendaStatus,
    pub created_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct GoalManager {
    pub goals: HashMap<String, Goal>,
    pub agenda: Vec<AgendaItem>,
}

impl GoalManager {
    pub fn new() -> Self {
        Self {
            goals: HashMap::new(),
            agenda: Vec::new(),
        }
    }

    pub fn add_goal(&mut self, goal: Goal) -> Result<(), String> {
        if goal.id.trim().is_empty() {
            return Err("Goal ID cannot be empty".into());
        }
        if self.goals.contains_key(&goal.id) {
            return Err(format!("Goal with ID '{}' already exists", goal.id));
        }
        if let Some(parent_id) = &goal.parent_goal
            && !self.goals.contains_key(parent_id)
        {
            return Err(format!("Parent goal '{parent_id}' does not exist"));
        }
        self.goals.insert(goal.id.clone(), goal);
        Ok(())
    }

    pub fn get_goal(&self, id: &str) -> Option<&Goal> {
        self.goals.get(id)
    }

    pub fn get_goal_mut(&mut self, id: &str) -> Option<&mut Goal> {
        self.goals.get_mut(id)
    }

    pub fn get_children(&self, parent_id: &str) -> Vec<&Goal> {
        self.goals
            .values()
            .filter(|g| g.parent_goal.as_deref() == Some(parent_id))
            .collect()
    }

    pub fn update_priority(&mut self, id: &str, priority: u32) -> Result<(), String> {
        let goal = self
            .goals
            .get_mut(id)
            .ok_or_else(|| format!("Goal '{id}' not found"))?;
        goal.priority = priority;
        Ok(())
    }

    pub fn cancel_goal(&mut self, id: &str) -> Result<(), String> {
        if !self.goals.contains_key(id) {
            return Err(format!("Goal '{id}' not found"));
        }
        let mut to_cancel = vec![id.to_string()];
        let mut idx = 0;
        while idx < to_cancel.len() {
            let current = to_cancel[idx].clone();
            idx += 1;
            for child in self.get_children(&current) {
                if !child.status.is_terminal() && !to_cancel.contains(&child.id) {
                    to_cancel.push(child.id.clone());
                }
            }
        }

        for target_id in &to_cancel {
            if let Some(g) = self.goals.get_mut(target_id)
                && !g.status.is_terminal()
            {
                g.status = GoalStatus::Cancelled;
            }
            for item in &mut self.agenda {
                if &item.goal_id == target_id
                    && (item.status == AgendaStatus::Ready
                        || item.status == AgendaStatus::Blocked
                        || item.status == AgendaStatus::Active)
                {
                    item.status = AgendaStatus::Cancelled;
                }
            }
        }
        Ok(())
    }

    pub fn evaluate_dependencies(&mut self) {
        let mut changed = true;
        let mut iterations = 0;
        while changed && iterations < 10 {
            changed = false;
            iterations += 1;
            let snapshot = self.goals.clone();
            for goal in self.goals.values_mut() {
                if goal.status == GoalStatus::Pending || goal.status == GoalStatus::Blocked {
                    let is_synthesis = goal
                        .completion_criteria
                        .iter()
                        .any(|c| matches!(c, CompletionCriterion::SynthesisCompleted));

                    if is_synthesis {
                        let all_deps_finished = !goal.dependencies.is_empty()
                            && goal.dependencies.iter().all(|dep_id| {
                                snapshot
                                    .get(dep_id)
                                    .map(|d| {
                                        d.status.is_terminal() || d.status == GoalStatus::Blocked
                                    })
                                    .unwrap_or(false)
                            });
                        if all_deps_finished && goal.status != GoalStatus::Active {
                            goal.status = GoalStatus::Active;
                            changed = true;
                        }
                        // Synthesis never blocks due to a failed dependency: it isolates failures
                        // and activates when all upstream subproblems have reached a terminal or blocked state.
                        continue;
                    }

                    let any_dep_failed = goal.dependencies.iter().any(|dep_id| {
                        snapshot
                            .get(dep_id)
                            .map(|d| {
                                d.status == GoalStatus::Failed || d.status == GoalStatus::Cancelled
                            })
                            .unwrap_or(false)
                    });
                    if any_dep_failed {
                        if goal.status != GoalStatus::Blocked {
                            goal.status = GoalStatus::Blocked;
                            changed = true;
                        }
                        continue;
                    }

                    let all_deps_completed = goal.dependencies.iter().all(|dep_id| {
                        snapshot
                            .get(dep_id)
                            .map(|d| d.status == GoalStatus::Completed)
                            .unwrap_or(false)
                    });

                    if all_deps_completed {
                        if goal.status != GoalStatus::Active {
                            goal.status = GoalStatus::Active;
                            changed = true;
                        }
                    } else if goal.status != GoalStatus::Blocked {
                        goal.status = GoalStatus::Blocked;
                        changed = true;
                    }
                }
            }
        }

        // Sync agenda items with goal states
        let goal_statuses: HashMap<String, GoalStatus> = self
            .goals
            .iter()
            .map(|(k, v)| (k.clone(), v.status.clone()))
            .collect();
        for item in &mut self.agenda {
            if let Some(g_status) = goal_statuses.get(&item.goal_id) {
                match g_status {
                    GoalStatus::Active => {
                        if item.status == AgendaStatus::Blocked {
                            item.status = AgendaStatus::Ready;
                        }
                    }
                    GoalStatus::Completed => {
                        if item.status != AgendaStatus::Completed {
                            item.status = AgendaStatus::Completed;
                        }
                    }
                    GoalStatus::Blocked => {
                        if item.status == AgendaStatus::Ready {
                            item.status = AgendaStatus::Blocked;
                        }
                    }
                    GoalStatus::Cancelled if item.status != AgendaStatus::Completed => {
                        item.status = AgendaStatus::Cancelled;
                    }
                    GoalStatus::Failed if item.status != AgendaStatus::Completed => {
                        item.status = AgendaStatus::Failed;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Verifies completion criteria before completing a goal.
    /// A goal can NEVER be completed solely by assertion; all criteria must evaluate to true.
    pub fn verify_and_complete_goal<F>(
        &mut self,
        id: &str,
        criterion_eval: F,
    ) -> Result<bool, String>
    where
        F: Fn(&CompletionCriterion) -> bool,
    {
        let goal = self
            .goals
            .get_mut(id)
            .ok_or_else(|| format!("Goal '{id}' not found"))?;

        if goal.status.is_terminal() {
            return Ok(goal.status == GoalStatus::Completed);
        }

        if goal.completion_criteria.is_empty() {
            return Err(format!(
                "Goal '{id}' has no completion criteria defined. Cannot complete without verified criteria."
            ));
        }

        let all_satisfied = goal.completion_criteria.iter().all(criterion_eval);

        if all_satisfied {
            goal.status = GoalStatus::Completed;
            goal.completed_at = Some(
                time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default(),
            );
            for item in &mut self.agenda {
                if item.goal_id == id && item.status != AgendaStatus::Completed {
                    item.status = AgendaStatus::Completed;
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn add_agenda_item(&mut self, item: AgendaItem) {
        self.agenda.push(item);
    }

    pub fn get_ready_agenda_items(&self) -> Vec<&AgendaItem> {
        self.agenda
            .iter()
            .filter(|a| {
                a.status == AgendaStatus::Ready
                    && self
                        .goals
                        .get(&a.goal_id)
                        .map(|g| g.status == GoalStatus::Active)
                        .unwrap_or(false)
            })
            .collect()
    }
}
