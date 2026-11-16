pub mod actions;
pub mod attention;
pub mod events;
pub mod executive;
pub mod goals;
pub mod policy;
pub mod state;
pub mod working_memory;

pub use actions::{ActionResult, ExecutiveAction, StopReason, SubgoalDef};
pub use attention::{AttentionDecisionRecord, AttentionEngine, CandidateGoalScore};
pub use events::{CognitiveEvent, CognitiveEventType};
pub use executive::{
    ExecutiveConfig, ExecutiveController, MetacognitiveEvaluation, MetacognitiveRecommendation,
};
pub use goals::{
    AgendaItem, AgendaStatus, CompletionCriterion, Goal, GoalManager, GoalStatus, Precondition,
};
pub use policy::{LoopDetector, validate_action_preconditions};
pub use state::{
    COGNITIVE_STATE_SCHEMA_VERSION, CognitiveState, ContradictionRecord, ExecutiveControllerState,
    ExecutivePhase, Hypothesis, InvestigationContext, KnowledgeItem, Provenance,
};
pub use working_memory::{
    DEFAULT_WORKING_MEMORY_CAPACITY, KnowledgeItemType, WorkingMemory, WorkingMemoryItem,
    WorkingMemorySnapshot,
};

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::contracts::EpistemicStatus;

    #[test]
    fn test_working_memory_capacity_and_eviction() {
        let mut wm = WorkingMemory::new(4);
        assert_eq!(wm.capacity(), 4);

        // Insert 4 items
        for i in 1..=4 {
            let item = WorkingMemoryItem::new(
                format!("item-{i}"),
                KnowledgeItemType::Observation,
                json!({"val": i}),
                EpistemicStatus::Uncertain,
            )
            .with_priority(10 * i as u32);
            assert!(wm.insert(item).is_ok());
        }
        assert_eq!(wm.len(), 4);

        // Pin item-1: must NEVER be evicted!
        assert!(wm.pin("item-1", true).is_ok());

        // Insert item-5: item-2 has the lowest retention score among unpinned items
        let item5 = WorkingMemoryItem::new(
            "item-5",
            KnowledgeItemType::Fact,
            json!({"val": 5}),
            EpistemicStatus::Calculated,
        )
        .with_priority(80);

        let evicted = wm
            .insert(item5)
            .expect("Insert should succeed with eviction");
        assert_eq!(
            evicted,
            Some("item-2".into()),
            "item-2 should be evicted because item-1 is pinned"
        );
        assert_eq!(wm.len(), 4);
        assert!(wm.contains("item-1"), "Pinned item must remain in memory");
        assert!(!wm.contains("item-2"));
        assert!(wm.contains("item-5"));
    }

    #[test]
    fn test_working_memory_retains_unresolved_contradictions() {
        let mut wm = WorkingMemory::new(4);
        // Fill memory with 3 normal items and 1 contradiction
        for i in 1..=3 {
            let item = WorkingMemoryItem::new(
                format!("fact-{i}"),
                KnowledgeItemType::Fact,
                json!({"fact": i}),
                EpistemicStatus::Calculated,
            )
            .with_priority(20);
            let _ = wm.insert(item);
        }

        let contradiction = WorkingMemoryItem::new(
            "contradiction-1",
            KnowledgeItemType::Observation,
            json!({"issue": "conflicting range measurements"}),
            EpistemicStatus::Contradicted,
        )
        .with_unresolved(true);
        let _ = wm.insert(contradiction);

        // Inserting 5th item should evict a normal fact, NOT the unresolved contradiction!
        let new_item = WorkingMemoryItem::new(
            "fact-4",
            KnowledgeItemType::Fact,
            json!({"fact": 4}),
            EpistemicStatus::Calculated,
        )
        .with_priority(90);

        let evicted = wm.insert(new_item).unwrap();
        assert_ne!(
            evicted,
            Some("contradiction-1".into()),
            "Unresolved contradictions must never be silently discarded"
        );
        assert!(wm.contains("contradiction-1"));
    }

    #[test]
    fn test_working_memory_snapshot_and_restoration() {
        let mut wm = WorkingMemory::new(10);
        let item = WorkingMemoryItem::new(
            "item-a",
            KnowledgeItemType::Calculation,
            json!({"result": 42}),
            EpistemicStatus::Verified,
        );
        let _ = wm.insert(item);
        wm.current_focus = Some("goal-1".into());

        let snapshot = wm.snapshot();
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.current_focus, Some("goal-1".into()));

        let mut restored = WorkingMemory::new(5);
        restored.restore_snapshot(snapshot);
        assert_eq!(restored.len(), 1);
        assert!(restored.contains("item-a"));
        assert_eq!(restored.current_focus, Some("goal-1".into()));
    }

    #[test]
    fn test_attention_prioritisation_and_tie_breaking() {
        let engine = AttentionEngine::new();
        let mut gm = GoalManager::new();
        let wm = WorkingMemory::new(10);

        let goal1 = Goal {
            id: "goal-1".into(),
            description: "Low priority active goal".into(),
            parent_goal: None,
            priority: 20,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::SubGoalsCompleted],
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: None,
        };
        let goal2 = Goal {
            id: "goal-2".into(),
            description: "High priority blocked goal".into(),
            parent_goal: None,
            priority: 95,
            status: GoalStatus::Blocked,
            dependencies: vec!["goal-x".into()],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::SubGoalsCompleted],
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: None,
        };
        let goal3 = Goal {
            id: "goal-3".into(),
            description: "High priority active goal".into(),
            parent_goal: None,
            priority: 85,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::SubGoalsCompleted],
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: None,
        };

        let _ = gm.add_goal(goal1);
        let _ = gm.add_goal(goal2);
        let _ = gm.add_goal(goal3);

        let decision = engine.select_focus(&gm, &wm, &[]);
        assert_eq!(
            decision.selected_goal_id,
            Some("goal-3".into()),
            "Attention must favor active ready goals over blocked goals, even if blocked has slightly higher base priority"
        );
    }

    #[test]
    fn test_goal_hierarchies_dependencies_and_cancellation() {
        let mut gm = GoalManager::new();

        let parent = Goal {
            id: "parent-1".into(),
            description: "Root investigation objective".into(),
            parent_goal: None,
            priority: 50,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::SubGoalsCompleted],
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: None,
        };
        let child1 = Goal {
            id: "child-1".into(),
            description: "Subgoal step 1".into(),
            parent_goal: Some("parent-1".into()),
            priority: 40,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::ToolExecuted {
                tool_name: "arithmetic".into(),
            }],
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: None,
        };
        let child2 = Goal {
            id: "child-2".into(),
            description: "Subgoal step 2 depends on step 1".into(),
            parent_goal: Some("parent-1".into()),
            priority: 40,
            status: GoalStatus::Pending,
            dependencies: vec!["child-1".into()],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::SubGoalsCompleted],
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: None,
        };

        assert!(gm.add_goal(parent).is_ok());
        assert!(gm.add_goal(child1).is_ok());
        assert!(gm.add_goal(child2).is_ok());

        assert_eq!(gm.get_children("parent-1").len(), 2);

        // Evaluate dependencies: child2 should be Blocked until child1 completes
        gm.evaluate_dependencies();
        assert_eq!(gm.get_goal("child-2").unwrap().status, GoalStatus::Blocked);

        // Cancel parent: recursive cancellation must cancel child1 and child2
        assert!(gm.cancel_goal("parent-1").is_ok());
        assert_eq!(
            gm.get_goal("parent-1").unwrap().status,
            GoalStatus::Cancelled
        );
        assert_eq!(
            gm.get_goal("child-1").unwrap().status,
            GoalStatus::Cancelled
        );
        assert_eq!(
            gm.get_goal("child-2").unwrap().status,
            GoalStatus::Cancelled
        );
    }

    #[test]
    fn test_goal_completion_enforces_criteria() {
        let mut gm = GoalManager::new();
        let goal = Goal {
            id: "verify-goal".into(),
            description: "Must verify projectile range calculation".into(),
            parent_goal: None,
            priority: 50,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::CalculationVerified {
                subject_id: "projectile-1".into(),
            }],
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: None,
        };
        assert!(gm.add_goal(goal).is_ok());

        // Attempting to complete when criteria return false must FAIL
        let completed = gm
            .verify_and_complete_goal("verify-goal", |_crit| false)
            .unwrap();
        assert!(!completed);
        assert_eq!(
            gm.get_goal("verify-goal").unwrap().status,
            GoalStatus::Active
        );

        // When criteria are satisfied, completion succeeds
        let completed_success = gm
            .verify_and_complete_goal("verify-goal", |_crit| true)
            .unwrap();
        assert!(completed_success);
        assert_eq!(
            gm.get_goal("verify-goal").unwrap().status,
            GoalStatus::Completed
        );
        assert!(gm.get_goal("verify-goal").unwrap().completed_at.is_some());
    }

    #[test]
    fn test_executive_controller_deterministic_execution_and_loop_prevention() {
        let mut controller = ExecutiveController::new(
            "inv-test-1",
            "Calculate projectile range",
            ExecutiveConfig {
                max_cycles: 10,
                max_actions: 20,
                timeout: Duration::from_secs(5),
                working_memory_capacity: 10,
            },
        );

        // Add a goal requiring projectile tool execution
        let goal = Goal {
            id: "proj-goal".into(),
            description: "Execute ideal projectile computation".into(),
            parent_goal: None,
            priority: 90,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::ToolExecuted {
                tool_name: "projectile".into(),
            }],
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: None,
        };
        assert!(controller.state.goals.add_goal(goal).is_ok());

        // Step 1: Controller should select ExecuteScientificTool for projectile
        let step1 = controller.step().unwrap();
        assert!(step1);

        assert_eq!(
            controller
                .state
                .executive
                .selected_action
                .as_ref()
                .unwrap()
                .name(),
            "execute_scientific_tool"
        );
        assert_eq!(
            controller.state.working_memory.len(),
            1,
            "Calculation item was inserted into working memory"
        );

        // Step 2: Goal criteria evaluated -> goal completed!
        let _ = controller.step().unwrap();
        assert_eq!(
            controller.state.goals.get_goal("proj-goal").unwrap().status,
            GoalStatus::Completed
        );
        assert!(controller.state.is_stopped());
        assert_eq!(
            controller.state.executive.stop_reason,
            Some(StopReason::AllGoalsCompleted)
        );
    }

    #[test]
    fn test_metacognitive_monitoring_detects_contradictions() {
        let mut controller = ExecutiveController::new(
            "inv-meta",
            "Evaluate contradictions",
            ExecutiveConfig::default(),
        );

        controller.state.record_contradiction(
            "contra-1",
            "Empirical power law contradicts theoretical derivation",
            vec!["fact-1".into(), "fact-2".into()],
        );

        assert_eq!(controller.state.unresolved_contradictions().len(), 1);
        let eval = controller.evaluate_metacognition();
        assert!(!eval.healthy);
        assert_eq!(
            eval.recommendation,
            MetacognitiveRecommendation::ResolveContradiction
        );
    }
}
