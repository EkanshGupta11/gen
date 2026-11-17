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
                max_concurrency: 1,
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
            content: json!({
                "tool": "projectile",
                "arguments": {"v0": 20.0, "angle": 45.0},
                "units": "meters",
                "assumptions": ["ideal vacuum", "flat ground"],
                "result": 38.33
            }),
            provenance: Provenance::new("scientific_tool"),
            epistemic_status: EpistemicStatus::Verified,
            related_goals: vec![],
            related_entities: vec![],
            timestamp: "2026-01-01T00:00:00Z".into(),
        };
        let k_item_b = KnowledgeItem {
            id: "item-b".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({
                "tool": "projectile",
                "arguments": {"v0": 20.0, "angle": 45.0},
                "units": "meters",
                "assumptions": ["ideal vacuum", "flat ground"],
                "result": 999.0
            }),
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

    #[test]
    fn test_multiple_verified_conflicting_evidence_requires_human_review() {
        let mut controller = ExecutiveController::new(
            "inv-multi-verified",
            "Test contradiction with conflicting verified items",
            ExecutiveConfig::default(),
        );

        controller.state.record_contradiction(
            "contra-conflicting-verified",
            "Two independent experiments verified conflicting values",
            vec!["verified-calc-1".into(), "verified-calc-2".into()],
        );

        let k1 = KnowledgeItem {
            id: "verified-calc-1".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({"tool": "projectile", "arguments": {"speed": 20.0, "angle_degrees": 35.0}, "output": {"range_m": 38.33}}),
            provenance: Provenance::new("scientific_tool").with_source_id("physics"),
            epistemic_status: EpistemicStatus::Verified,
            related_goals: vec![],
            related_entities: vec![],
            timestamp: "2026-11-17T00:00:00Z".into(),
        };
        let k2 = KnowledgeItem {
            id: "verified-calc-2".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({"tool": "projectile", "arguments": {"speed": 20.0, "angle_degrees": 35.0}, "output": {"range_m": 42.15}}),
            provenance: Provenance::new("scientific_tool").with_source_id("physics"),
            epistemic_status: EpistemicStatus::Verified,
            related_goals: vec![],
            related_entities: vec![],
            timestamp: "2026-11-17T00:00:00Z".into(),
        };
        controller.state.add_knowledge_item(k1);
        controller.state.add_knowledge_item(k2);

        let resolve_action = ExecutiveAction::ResolveContradiction {
            contradiction_id: "contra-conflicting-verified".into(),
            resolution_strategy: "deterministic_arbitration".into(),
        };

        let outcome = controller.execute_action(&resolve_action);

        // Multiple conflicting verified evidence MUST NOT automatically resolve!
        assert_eq!(outcome.status, ActionExecutionStatus::Uncertain);
        assert!(!controller.state.contradictions[0].resolved);
        let res = controller.state.contradictions[0]
            .resolution
            .as_ref()
            .expect("Resolution record must be created");
        assert_eq!(res.outcome, ResolutionOutcome::RequiresHumanReview);
        assert!(
            res.proposed_resolution
                .contains("Multiple conflicting verified evidence items")
        );
    }

    #[test]
    fn test_incomparable_evidence_units_or_assumptions_requires_human_review() {
        let mut controller = ExecutiveController::new(
            "inv-incomparable",
            "Test incomparability arbitration",
            ExecutiveConfig::default(),
        );

        // Case 1: Conflicting Physical Assumptions (ideal vacuum vs air resistance)
        controller.state.record_contradiction(
            "contra-assumption-mismatch",
            "Calculation in vacuum vs calculation in fluid",
            vec!["calc-vacuum".into(), "calc-air".into()],
        );

        let v_item = KnowledgeItem {
            id: "calc-vacuum".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({
                "tool": "projectile",
                "arguments": {"speed": 20.0, "angle_degrees": 35.0},
                "units": "meters",
                "assumptions": ["ideal vacuum", "no drag"],
                "output": {"range_m": 38.33}
            }),
            provenance: Provenance::new("scientific_tool").with_source_id("physics"),
            epistemic_status: EpistemicStatus::Verified,
            related_goals: vec![],
            related_entities: vec![],
            timestamp: "2026-11-17T00:00:00Z".into(),
        };
        let o_item = KnowledgeItem {
            id: "calc-air".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({
                "tool": "projectile",
                "arguments": {"speed": 20.0, "angle_degrees": 35.0},
                "units": "meters",
                "assumptions": ["air resistance", "turbulent drag"],
                "output": {"range_m": 29.50}
            }),
            provenance: Provenance::new("specialist_report").with_source_id("physics"),
            epistemic_status: EpistemicStatus::Uncertain,
            related_goals: vec![],
            related_entities: vec![],
            timestamp: "2026-11-17T00:00:00Z".into(),
        };
        controller.state.add_knowledge_item(v_item);
        controller.state.add_knowledge_item(o_item);

        let outcome = controller.execute_action(&ExecutiveAction::ResolveContradiction {
            contradiction_id: "contra-assumption-mismatch".into(),
            resolution_strategy: "assumption_aware_arbitration".into(),
        });

        assert_eq!(outcome.status, ActionExecutionStatus::Uncertain);
        assert!(!controller.state.contradictions[0].resolved);
        let res = controller.state.contradictions[0]
            .resolution
            .as_ref()
            .unwrap();
        assert_eq!(res.outcome, ResolutionOutcome::RequiresHumanReview);
        assert!(
            res.proposed_resolution
                .contains("air resistance vs ideal vacuum")
        );

        // Case 2: Unit mismatch
        controller.state.record_contradiction(
            "contra-unit-mismatch",
            "Meters vs feet mismatch",
            vec!["calc-meters".into(), "calc-feet".into()],
        );
        let m_item = KnowledgeItem {
            id: "calc-meters".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({
                "tool": "projectile",
                "arguments": {"speed": 20.0, "angle_degrees": 35.0},
                "units": "meters",
                "assumptions": ["ideal vacuum"],
                "output": {"range": 38.33}
            }),
            provenance: Provenance::new("scientific_tool").with_source_id("physics"),
            epistemic_status: EpistemicStatus::Verified,
            related_goals: vec![],
            related_entities: vec![],
            timestamp: "2026-11-17T00:00:00Z".into(),
        };
        let f_item = KnowledgeItem {
            id: "calc-feet".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({
                "tool": "projectile",
                "arguments": {"speed": 20.0, "angle_degrees": 35.0},
                "units": "feet",
                "assumptions": ["ideal vacuum"],
                "output": {"range": 125.75}
            }),
            provenance: Provenance::new("specialist_report").with_source_id("physics"),
            epistemic_status: EpistemicStatus::Uncertain,
            related_goals: vec![],
            related_entities: vec![],
            timestamp: "2026-11-17T00:00:00Z".into(),
        };
        controller.state.add_knowledge_item(m_item);
        controller.state.add_knowledge_item(f_item);

        let outcome2 = controller.execute_action(&ExecutiveAction::ResolveContradiction {
            contradiction_id: "contra-unit-mismatch".into(),
            resolution_strategy: "unit_aware_arbitration".into(),
        });
        assert_eq!(outcome2.status, ActionExecutionStatus::Uncertain);
        assert!(!controller.state.contradictions[1].resolved);
        let res2 = controller.state.contradictions[1]
            .resolution
            .as_ref()
            .unwrap();
        assert_eq!(res2.outcome, ResolutionOutcome::RequiresHumanReview);
        assert!(res2.proposed_resolution.contains("Incompatible units"));
    }

    #[test]
    fn test_correct_specialist_provenance_preserved() {
        use crate::cognition::executor::resolve_specialist_for_tool_execution;
        use crate::contracts::Specialist;

        let mut controller = ExecutiveController::new(
            "inv-provenance",
            "Test specialist provenance preservation",
            ExecutiveConfig::default(),
        );

        // Goal 1: Physics goal with CompletionCriterion::SpecialistReportReceived
        let goal_physics = Goal {
            id: "goal-phys".into(),
            description: "Analyze kinetic ballistics".into(),
            parent_goal: None,
            priority: 80,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::SpecialistReportReceived {
                specialist: "physics".into(),
            }],
            created_at: "2026-11-17T00:00:00Z".into(),
            completed_at: None,
        };
        controller.state.goals.add_goal(goal_physics).unwrap();

        let resolved_phys =
            resolve_specialist_for_tool_execution("goal-phys", "projectile", &controller.state);
        assert_eq!(
            resolved_phys,
            Specialist::Physics,
            "Physics goal must resolve to Specialist::Physics"
        );

        // Goal 2: Chemistry subproblem by description
        let goal_chem = Goal {
            id: "goal-chem".into(),
            description: "Perform Chemistry molecular weight calculation".into(),
            parent_goal: None,
            priority: 70,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![],
            created_at: "2026-11-17T00:00:00Z".into(),
            completed_at: None,
        };
        controller.state.goals.add_goal(goal_chem).unwrap();

        let resolved_chem =
            resolve_specialist_for_tool_execution("goal-chem", "arithmetic", &controller.state);
        assert_eq!(
            resolved_chem,
            Specialist::Chemistry,
            "Chemistry description must resolve to Specialist::Chemistry"
        );

        // Action execution assigns proper provenance and specialist in KnowledgeItem
        let action = ExecutiveAction::ExecuteScientificTool {
            tool: "projectile".into(),
            arguments: json!({"speed": 20.0, "angle_degrees": 35.0}),
            goal_id: "goal-phys".into(),
        };
        let outcome = controller.execute_action(&action);
        assert!(outcome.success);

        let k_item = controller
            .state
            .knowledge_items
            .values()
            .find(|k| k.related_goals.contains(&"goal-phys".to_string()))
            .expect("Knowledge item must be created for physics tool");

        assert_eq!(
            k_item.provenance.source_id.as_deref(),
            Some("physics"),
            "Provenance source_id must be 'physics', NOT hardcoded 'mathematics'"
        );
        assert_eq!(
            k_item
                .content
                .get("specialist")
                .and_then(serde_json::Value::as_str),
            Some("physics")
        );
    }

    #[test]
    fn test_concurrency_behaviour_serial_vs_parallel() {
        let mut controller = ExecutiveController::new(
            "inv-concurrency",
            "Test serial vs parallel agenda scheduling",
            ExecutiveConfig {
                max_concurrency: 2,
                ..Default::default()
            },
        );

        // Add 2 independent goals
        let g1 = Goal {
            id: "g1".into(),
            description: "Independent subproblem 1".into(),
            parent_goal: None,
            priority: 50,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::SpecialistReportReceived {
                specialist: "physics".into(),
            }],
            created_at: "2026-11-17T00:00:00Z".into(),
            completed_at: None,
        };
        let g2 = Goal {
            id: "g2".into(),
            description: "Independent subproblem 2".into(),
            parent_goal: None,
            priority: 50,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::SpecialistReportReceived {
                specialist: "mathematics".into(),
            }],
            created_at: "2026-11-17T00:00:00Z".into(),
            completed_at: None,
        };
        controller.state.goals.add_goal(g1).unwrap();
        controller.state.goals.add_goal(g2).unwrap();

        controller
            .state
            .goals
            .add_agenda_item(crate::cognition::goals::AgendaItem {
                id: "ag1".into(),
                goal_id: "g1".into(),
                action: ExecutiveAction::RequestSpecialistAnalysis {
                    specialist: crate::contracts::Specialist::Physics,
                    task: "Phys task".into(),
                    goal_id: "g1".into(),
                },
                status: crate::cognition::goals::AgendaStatus::Ready,
                created_at: "2026-11-17T00:00:00Z".into(),
            });
        controller
            .state
            .goals
            .add_agenda_item(crate::cognition::goals::AgendaItem {
                id: "ag2".into(),
                goal_id: "g2".into(),
                action: ExecutiveAction::RequestSpecialistAnalysis {
                    specialist: crate::contracts::Specialist::Mathematics,
                    task: "Math task".into(),
                    goal_id: "g2".into(),
                },
                status: crate::cognition::goals::AgendaStatus::Ready,
                created_at: "2026-11-17T00:00:00Z".into(),
            });

        // 1. Under serial limit = 1: get_concurrent_ready_actions returns empty
        let serial_batch = controller.get_concurrent_ready_actions(1);
        assert!(
            serial_batch.is_empty(),
            "Serial mode (limit=1) must not return parallel batch"
        );

        // 2. Under parallel limit = 2: returns both independent actions
        let parallel_batch = controller.get_concurrent_ready_actions(2);
        assert_eq!(
            parallel_batch.len(),
            2,
            "Parallel mode (limit=2) must schedule both independent actions"
        );

        // 3. If g2 depends on g1, dependent action must NOT be scheduled in parallel batch
        controller
            .state
            .goals
            .goals
            .get_mut("g2")
            .unwrap()
            .dependencies = vec!["g1".into()];
        let dependent_batch = controller.get_concurrent_ready_actions(2);
        assert!(
            dependent_batch.is_empty(),
            "Actions with dependency conflicts must not be executed concurrently"
        );
    }

    #[test]
    fn test_contradiction_comparability_fails_closed_on_missing_or_mismatched_metadata() {
        use crate::cognition::state::check_contradiction_comparability;

        let base_verified = KnowledgeItem {
            id: "v1".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({
                "tool": "projectile",
                "arguments": {"v0": 25.0, "angle": 45.0},
                "units": "meters",
                "assumptions": ["ideal vacuum", "flat ground"],
                "result": 63.71
            }),
            provenance: Provenance::new("tool"),
            epistemic_status: EpistemicStatus::Verified,
            related_goals: vec![],
            related_entities: vec![],
            timestamp: "2026-11-17T00:00:00Z".into(),
        };

        let mut other = base_verified.clone();
        other.id = "o1".into();
        other.content["result"] = json!(999.0);
        assert!(
            check_contradiction_comparability(&base_verified, &other).is_ok(),
            "Exact condition match must be comparable"
        );

        // 1. Missing tool
        let mut missing_tool = other.clone();
        missing_tool.content.as_object_mut().unwrap().remove("tool");
        let err = check_contradiction_comparability(&base_verified, &missing_tool).unwrap_err();
        assert!(err.contains("Tool or scientific operation specification missing"));

        // 2. Mismatched tool
        let mut mismatched_tool = other.clone();
        mismatched_tool.content["tool"] = json!("pendulum");
        let err = check_contradiction_comparability(&base_verified, &mismatched_tool).unwrap_err();
        assert!(err.contains("Different tools used"));

        // 3. Missing arguments
        let mut missing_args = other.clone();
        missing_args
            .content
            .as_object_mut()
            .unwrap()
            .remove("arguments");
        let err = check_contradiction_comparability(&base_verified, &missing_args).unwrap_err();
        assert!(err.contains("Condition parameters/arguments missing"));

        // 4. Parameter value mismatch
        let mut mismatched_param_val = other.clone();
        mismatched_param_val.content["arguments"]["v0"] = json!(50.0);
        let err =
            check_contradiction_comparability(&base_verified, &mismatched_param_val).unwrap_err();
        assert!(err.contains("Condition parameter mismatch for 'v0'"));

        // 5. Parameter set mismatch
        let mut mismatched_param_key = other.clone();
        mismatched_param_key.content["arguments"] = json!({"v0": 25.0, "elevation": 10.0});
        let err =
            check_contradiction_comparability(&base_verified, &mismatched_param_key).unwrap_err();
        assert!(err.contains("Mismatched condition parameters"));

        // 6. Missing units
        let mut missing_units = other.clone();
        missing_units
            .content
            .as_object_mut()
            .unwrap()
            .remove("units");
        let err = check_contradiction_comparability(&base_verified, &missing_units).unwrap_err();
        assert!(err.contains("Units specification missing"));

        // 7. Incompatible units
        let mut mismatched_units = other.clone();
        mismatched_units.content["units"] = json!("feet");
        let err = check_contradiction_comparability(&base_verified, &mismatched_units).unwrap_err();
        assert!(err.contains("Incompatible units"));

        // 8. Missing assumptions
        let mut missing_assumptions = other.clone();
        missing_assumptions
            .content
            .as_object_mut()
            .unwrap()
            .remove("assumptions");
        let err =
            check_contradiction_comparability(&base_verified, &missing_assumptions).unwrap_err();
        assert!(err.contains("Explicit scientific assumptions missing"));

        // 9. Conflicting assumptions: air resistance vs vacuum
        let mut drag_assumptions = other.clone();
        drag_assumptions.content["assumptions"] = json!(["air drag", "viscous friction"]);
        let err = check_contradiction_comparability(&base_verified, &drag_assumptions).unwrap_err();
        assert!(err.contains("air resistance vs ideal vacuum"));

        // 10. Conflicting assumptions: relativistic vs classical
        let mut rel_v = base_verified.clone();
        rel_v.content["assumptions"] = json!(["special relativity", "lorentz contraction"]);
        let mut class_o = other.clone();
        class_o.content["assumptions"] = json!(["classical mechanics", "galilean"]);
        let err = check_contradiction_comparability(&rel_v, &class_o).unwrap_err();
        assert!(err.contains("Conflicting model assumptions"));

        // 11. Missing result
        let mut missing_result = other.clone();
        missing_result
            .content
            .as_object_mut()
            .unwrap()
            .remove("result");
        let err = check_contradiction_comparability(&base_verified, &missing_result).unwrap_err();
        assert!(err.contains("Output result field missing"));
    }

    #[test]
    fn test_step_prepare_batch_enforces_authoritative_lifecycle_and_loop_detection() {
        let mut controller = ExecutiveController::new(
            "inv-batch-lifecycle",
            "Test batch lifecycle",
            ExecutiveConfig {
                max_cycles: 5,
                max_actions: 10,
                timeout: std::time::Duration::from_secs(10),
                working_memory_capacity: 10,
                max_concurrency: 3,
            },
        );

        // Budget reservation enforcement: remaining budget set to 2
        controller.state.executive.remaining_action_budget = 2;

        let make_goal = |id: &str, desc: &str| Goal {
            id: id.into(),
            description: desc.into(),
            parent_goal: None,
            priority: 50,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![],
            created_at: "2026-11-17T00:00:00Z".into(),
            completed_at: None,
        };

        controller
            .state
            .goals
            .add_goal(make_goal("g1", "Goal 1"))
            .unwrap();
        controller
            .state
            .goals
            .add_goal(make_goal("g2", "Goal 2"))
            .unwrap();
        controller
            .state
            .goals
            .add_goal(make_goal("g3", "Goal 3"))
            .unwrap();

        controller
            .state
            .goals
            .add_agenda_item(crate::cognition::goals::AgendaItem {
                id: "ag1".into(),
                goal_id: "g1".into(),
                action: ExecutiveAction::RequestSpecialistAnalysis {
                    specialist: crate::contracts::Specialist::Physics,
                    task: "T1".into(),
                    goal_id: "g1".into(),
                },
                status: crate::cognition::goals::AgendaStatus::Ready,
                created_at: "2026-11-17T00:00:00Z".into(),
            });
        controller
            .state
            .goals
            .add_agenda_item(crate::cognition::goals::AgendaItem {
                id: "ag2".into(),
                goal_id: "g2".into(),
                action: ExecutiveAction::RequestSpecialistAnalysis {
                    specialist: crate::contracts::Specialist::Mathematics,
                    task: "T2".into(),
                    goal_id: "g2".into(),
                },
                status: crate::cognition::goals::AgendaStatus::Ready,
                created_at: "2026-11-17T00:00:00Z".into(),
            });
        controller
            .state
            .goals
            .add_agenda_item(crate::cognition::goals::AgendaItem {
                id: "ag3".into(),
                goal_id: "g3".into(),
                action: ExecutiveAction::RequestSpecialistAnalysis {
                    specialist: crate::contracts::Specialist::Chemistry,
                    task: "T3".into(),
                    goal_id: "g3".into(),
                },
                status: crate::cognition::goals::AgendaStatus::Ready,
                created_at: "2026-11-17T00:00:00Z".into(),
            });

        // Batch capacity capped at remaining_action_budget (2), despite max_concurrency=3
        let batch = controller.step_prepare_batch(3).unwrap();
        assert_eq!(
            batch.len(),
            2,
            "Batch must be clamped to remaining action budget of 2"
        );

        // Loop detection on batch actions: repeating the same action triggers MetacognitiveAbort
        let repeating_action = batch[0].clone();
        for _ in 0..4 {
            let _ = controller
                .loop_detector
                .record_and_check_loop(&repeating_action);
        }

        let loop_result = controller.step_prepare_batch(3).unwrap();
        assert!(loop_result.is_empty());
        assert!(matches!(
            controller.state.executive.stop_reason,
            Some(StopReason::MetacognitiveAbort { .. })
        ));
    }

    #[test]
    fn test_cancelled_goal_semantics_prevent_false_completion() {
        let mut controller = ExecutiveController::new(
            "inv-cancel-semantics",
            "Test honest cancellation",
            ExecutiveConfig::default(),
        );

        let g1 = Goal {
            id: "g1".into(),
            description: "Cancelled goal".into(),
            parent_goal: None,
            priority: 50,
            status: GoalStatus::Cancelled,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![],
            created_at: "2026-11-17T00:00:00Z".into(),
            completed_at: None,
        };
        let g2 = Goal {
            id: "g2".into(),
            description: "Completed goal".into(),
            parent_goal: None,
            priority: 50,
            status: GoalStatus::Completed,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![],
            created_at: "2026-11-17T00:00:00Z".into(),
            completed_at: None,
        };

        controller.state.goals.add_goal(g1).unwrap();
        controller.state.goals.add_goal(g2).unwrap();

        let batch = controller.step_prepare_batch(2).unwrap();
        assert!(
            batch.is_empty(),
            "Terminal goals must produce empty prepare batch"
        );
        assert_eq!(
            controller.state.executive.stop_reason,
            Some(StopReason::UserCancelled),
            "Partially cancelled goals must stop with UserCancelled, NEVER AllGoalsCompleted"
        );
    }

    #[test]
    fn test_failed_action_does_not_satisfy_another_goal() {
        let mut controller = ExecutiveController::new(
            "inv-goal-isolation",
            "Test goal completion isolation",
            ExecutiveConfig::default(),
        );

        let g1 = Goal {
            id: "g1".into(),
            description: "Physics goal".into(),
            parent_goal: None,
            priority: 50,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::ToolExecuted {
                tool_name: "projectile".into(),
            }],
            created_at: "2026-11-17T00:00:00Z".into(),
            completed_at: None,
        };

        let g2 = Goal {
            id: "g2".into(),
            description: "Math goal".into(),
            parent_goal: None,
            priority: 50,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::ToolExecuted {
                tool_name: "arithmetic".into(),
            }],
            created_at: "2026-11-17T00:00:00Z".into(),
            completed_at: None,
        };

        controller.state.goals.add_goal(g1).unwrap();
        controller.state.goals.add_goal(g2).unwrap();

        // Add a knowledge item strictly related to g1
        let k_item = KnowledgeItem {
            id: "calc-1".into(),
            item_type: KnowledgeItemType::Calculation,
            content: json!({"tool": "projectile", "result": 42.0}),
            provenance: Provenance::new("tool"),
            epistemic_status: EpistemicStatus::Verified,
            related_goals: vec!["g1".into()],
            related_entities: vec![],
            timestamp: "2026-11-17T00:00:00Z".into(),
        };
        controller.state.add_knowledge_item(k_item);

        controller.evaluate_goal_completions();

        assert_eq!(
            controller.state.goals.get_goal("g1").unwrap().status,
            GoalStatus::Completed,
            "Target goal g1 should be completed"
        );
        assert_eq!(
            controller.state.goals.get_goal("g2").unwrap().status,
            GoalStatus::Active,
            "Unrelated goal g2 must NOT be satisfied by g1's tool execution"
        );
    }
}
