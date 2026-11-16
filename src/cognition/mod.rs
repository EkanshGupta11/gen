pub mod actions;
pub mod attention;
pub mod events;
pub mod executive;
pub mod executor;
pub mod goals;
pub mod policy;
pub mod runtime;
pub mod state;
pub mod working_memory;

pub use actions::{ActionExecutionStatus, ActionResult, ExecutiveAction, StopReason, SubgoalDef};
pub use attention::{AttentionDecisionRecord, AttentionEngine, CandidateGoalScore};
pub use events::{CognitiveEvent, CognitiveEventType};
pub use executive::{
    ExecutiveConfig, ExecutiveController, MetacognitiveEvaluation, MetacognitiveRecommendation,
};
pub use executor::AsyncActionExecutor;
pub use goals::{
    AgendaItem, AgendaStatus, CompletionCriterion, Goal, GoalManager, GoalStatus, Precondition,
};
pub use policy::{ALLOWED_SCIENTIFIC_TOOLS, LoopDetector, validate_action_preconditions};
pub use runtime::CognitiveRuntime;
pub use state::{
    COGNITIVE_STATE_SCHEMA_VERSION, CognitiveState, ContradictionRecord, ContradictionResolution,
    ExecutiveControllerState, ExecutivePhase, Hypothesis, InvestigationContext, KnowledgeItem,
    Provenance, ResolutionOutcome,
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

    #[test]
    fn test_unimplemented_and_unsupported_actions_do_not_report_success() {
        let mut controller = ExecutiveController::new(
            "inv-unsupported",
            "Test unsupported actions",
            ExecutiveConfig::default(),
        );

        // Action not implemented in synchronous runner
        let action = ExecutiveAction::RetrieveKnowledge {
            query: "quantum electrodynamics".into(),
            domain: "physics".into(),
        };

        // When executed, must return Unsupported status with success == false
        let outcome = controller.execute_action(&action);
        assert!(!outcome.success);
        assert_eq!(outcome.status, ActionExecutionStatus::Unsupported);
        assert!(outcome.error.is_some());
    }

    #[test]
    fn test_verification_cannot_be_bypassed_without_independent_checker() {
        let mut controller = ExecutiveController::new(
            "inv-verify-bypass",
            "Verify calculation integrity",
            ExecutiveConfig::default(),
        );

        // Add a goal and an uncertain calculation knowledge item with corrupt output
        let goal_id = "calc-goal".to_string();
        let k_item = KnowledgeItem {
            id: "calc-corrupt".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({
                "tool": "projectile",
                "arguments": {"speed": 20.0, "angle_degrees": 35.0},
                "output": {"range_m": 9999.0} // Corrupted!
            }),
            provenance: Provenance::new("scientific_tool").with_source_id("projectile"),
            epistemic_status: EpistemicStatus::Uncertain,
            related_goals: vec![goal_id.clone()],
            related_entities: vec![],
            timestamp: "2026-01-01T00:00:00Z".into(),
        };
        controller.state.add_knowledge_item(k_item);

        let verify_action = ExecutiveAction::VerifyCalculation {
            tool_id: "calc-corrupt".into(),
            goal_id: goal_id.clone(),
        };

        let outcome = controller.execute_action(&verify_action);
        assert!(
            !outcome.success,
            "Corrupted calculation must NOT succeed verification"
        );
        assert_eq!(outcome.status, ActionExecutionStatus::Contradicted);

        // Epistemic status in state must be Contradicted, NEVER Verified!
        let updated_item = controller
            .state
            .knowledge_items
            .get("calc-corrupt")
            .unwrap();
        assert_eq!(updated_item.epistemic_status, EpistemicStatus::Contradicted);
        assert_eq!(controller.state.unresolved_contradictions().len(), 1);
    }

    #[test]
    fn test_contradiction_resolution_requires_defensible_verified_evidence() {
        let mut controller = ExecutiveController::new(
            "inv-contra-resolve",
            "Test evidence-based contradiction resolution",
            ExecutiveConfig::default(),
        );

        // Case 1: Contradiction with NO verified evidence cannot be resolved automatically
        controller.state.record_contradiction(
            "contra-unverified",
            "Uncertain claim conflict",
            vec!["item-a".into(), "item-b".into()],
        );

        let resolve_action = ExecutiveAction::ResolveContradiction {
            contradiction_id: "contra-unverified".into(),
            resolution_strategy: "deterministic_verification_arbitration".into(),
        };

        let outcome1 = controller.execute_action(&resolve_action);
        assert!(!outcome1.success);
        assert_eq!(outcome1.status, ActionExecutionStatus::Uncertain);
        assert!(!controller.state.contradictions[0].resolved);
        assert_eq!(
            controller.state.contradictions[0]
                .resolution
                .as_ref()
                .unwrap()
                .outcome,
            ResolutionOutcome::RequiresHumanReview
        );

        // Case 2: Add verified evidence for item-a and contradicted for item-b
        let k_item_a = KnowledgeItem {
            id: "item-a".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({"tool": "projectile", "result": 38.33}),
            provenance: Provenance::new("scientific_tool"),
            epistemic_status: EpistemicStatus::Verified,
            related_goals: vec![],
            related_entities: vec![],
            timestamp: "2026-01-01T00:00:00Z".into(),
        };
        let k_item_b = KnowledgeItem {
            id: "item-b".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({"tool": "projectile", "result": 999.0}),
            provenance: Provenance::new("model_claim"),
            epistemic_status: EpistemicStatus::Contradicted,
            related_goals: vec![],
            related_entities: vec![],
            timestamp: "2026-01-01T00:00:00Z".into(),
        };
        controller.state.add_knowledge_item(k_item_a);
        controller.state.add_knowledge_item(k_item_b);

        let outcome2 = controller.execute_action(&resolve_action);
        assert!(outcome2.success);
        assert_eq!(outcome2.status, ActionExecutionStatus::Succeeded);
        assert!(controller.state.contradictions[0].resolved);
        assert_eq!(
            controller.state.contradictions[0]
                .resolution
                .as_ref()
                .unwrap()
                .outcome,
            ResolutionOutcome::Resolved
        );
    }

    #[test]
    fn test_budget_and_loop_history_preserved_across_from_existing_state() {
        let mut state = CognitiveState::new("inv-budget", "Test budget preservation", 16, 50);
        state.executive.total_actions_executed = 15;
        state.executive.remaining_action_budget = 35;
        state.executive.loop_detection_history =
            vec!["execute_scientific_tool:Some(\"goal-1\")".into()];

        let controller =
            ExecutiveController::from_existing_state(state, ExecutiveConfig::default());
        assert_eq!(controller.total_actions_executed, 15);
        assert_eq!(controller.state.executive.remaining_action_budget, 35);
        assert_eq!(
            controller.state.executive.loop_detection_history,
            vec!["execute_scientific_tool:Some(\"goal-1\")"]
        );
    }
}
