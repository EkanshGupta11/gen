use serde::{Deserialize, Serialize};

use crate::cognition::{
    goals::{GoalManager, GoalStatus},
    working_memory::{KnowledgeItemType, WorkingMemory},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CandidateGoalScore {
    pub goal_id: String,
    pub base_priority: f64,
    pub readiness_score: f64,
    pub contradiction_bonus: f64,
    pub uncertainty_bonus: f64,
    pub missing_info_bonus: f64,
    pub total_score: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AttentionDecisionRecord {
    pub selected_goal_id: Option<String>,
    pub focused_item_ids: Vec<String>,
    pub candidate_scores: Vec<CandidateGoalScore>,
    pub rationale: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, Default)]
pub struct AttentionEngine;

impl AttentionEngine {
    pub fn new() -> Self {
        Self
    }

    /// Evaluates candidate goals and working memory items to select the focus of attention.
    /// Uses a documented, deterministic scoring policy with deterministic tie-breaking.
    pub fn select_focus(
        &self,
        goal_manager: &GoalManager,
        working_memory: &WorkingMemory,
        unknown_variables: &[String],
    ) -> AttentionDecisionRecord {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        let contradictions = working_memory.get_unresolved_contradictions();
        let contradiction_goal_ids: std::collections::HashSet<_> = contradictions
            .iter()
            .flat_map(|c| c.related_goals.iter().cloned())
            .collect();

        let mut candidate_scores = Vec::new();

        for goal in goal_manager.goals.values() {
            if goal.status.is_terminal() {
                continue;
            }

            let base_priority = (goal.priority.min(100) as f64) / 100.0;
            let readiness_score = if goal.status == GoalStatus::Active {
                1.0
            } else {
                0.0
            };

            let contradiction_bonus = if contradiction_goal_ids.contains(&goal.id) {
                1.0
            } else {
                0.0
            };

            let uncertainty_bonus = if goal.completion_criteria.iter().any(|c| {
                matches!(
                    c,
                    crate::cognition::goals::CompletionCriterion::HypothesesTested
                )
            }) {
                0.8
            } else {
                0.0
            };

            let missing_info_bonus = if unknown_variables.iter().any(|var| {
                goal.description
                    .to_lowercase()
                    .contains(&var.to_lowercase())
            }) {
                0.6
            } else {
                0.0
            };

            let total_score = (readiness_score * 40.0)
                + (contradiction_bonus * 25.0)
                + (base_priority * 20.0)
                + (uncertainty_bonus * 10.0)
                + (missing_info_bonus * 5.0);

            candidate_scores.push((
                goal,
                CandidateGoalScore {
                    goal_id: goal.id.clone(),
                    base_priority,
                    readiness_score,
                    contradiction_bonus,
                    uncertainty_bonus,
                    missing_info_bonus,
                    total_score,
                },
            ));
        }

        // Deterministic ranking and tie-breaking:
        // 1. Total score (descending)
        // 2. Goal priority (descending)
        // 3. Goal creation timestamp (ascending)
        // 4. Goal ID lexicographical (ascending)
        candidate_scores.sort_by(|(g_a, s_a), (g_b, s_b)| {
            s_b.total_score
                .partial_cmp(&s_a.total_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| g_b.priority.cmp(&g_a.priority))
                .then_with(|| g_a.created_at.cmp(&g_b.created_at))
                .then_with(|| g_a.id.cmp(&g_b.id))
        });

        let selected_goal = candidate_scores.first().map(|(g, _)| (*g).clone());
        let scores_summary: Vec<CandidateGoalScore> =
            candidate_scores.into_iter().map(|(_, s)| s).collect();

        // Select focused working memory items
        let mut focused_item_ids = Vec::new();
        if let Some(ref goal) = selected_goal {
            for item in working_memory.get_active_items(0.3) {
                if item.related_goals.contains(&goal.id)
                    || item.pinned
                    || item.unresolved
                    || item.item_type == KnowledgeItemType::Constraint
                {
                    focused_item_ids.push(item.id.clone());
                }
            }
        }

        let rationale = match &selected_goal {
            Some(g) => {
                if contradiction_goal_ids.contains(&g.id) {
                    format!(
                        "Priority attention directed to goal '{}' to resolve active contradiction.",
                        g.id
                    )
                } else if g.status == GoalStatus::Active {
                    format!(
                        "Attention focused on highest-scoring active goal '{}' (priority {}).",
                        g.id, g.priority
                    )
                } else {
                    format!("Attention directed to goal '{}'.", g.id)
                }
            }
            None => "No active or pending goals eligible for attention focus.".into(),
        };

        AttentionDecisionRecord {
            selected_goal_id: selected_goal.map(|g| g.id),
            focused_item_ids,
            candidate_scores: scores_summary,
            rationale,
            timestamp: now,
        }
    }
}
