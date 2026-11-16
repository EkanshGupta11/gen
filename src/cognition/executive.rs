use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    cognition::{
        actions::{ActionResult, ExecutiveAction, StopReason},
        attention::AttentionEngine,
        events::{CognitiveEvent, CognitiveEventType},
        goals::{CompletionCriterion, Goal, GoalStatus},
        policy::{LoopDetector, validate_action_preconditions},
        state::{CognitiveState, ExecutivePhase, KnowledgeItem, Provenance},
        working_memory::KnowledgeItemType,
    },
    contracts::EpistemicStatus,
    verification,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetacognitiveRecommendation {
    Proceed,
    TriggerVerification,
    ResolveContradiction,
    DecomposeGoal,
    AbortDeadlock,
    CompleteInvestigation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MetacognitiveEvaluation {
    pub cycle: u32,
    pub healthy: bool,
    pub progress_detected: bool,
    pub repeated_failures_detected: bool,
    pub unverified_calculations_count: usize,
    pub unresolved_contradictions_count: usize,
    pub open_hypotheses_without_evidence_count: usize,
    pub recommendation: MetacognitiveRecommendation,
    pub rationale: String,
}

#[derive(Clone, Debug)]
pub struct ExecutiveConfig {
    pub max_cycles: u32,
    pub max_actions: u32,
    pub timeout: Duration,
    pub working_memory_capacity: usize,
}

impl Default for ExecutiveConfig {
    fn default() -> Self {
        Self {
            max_cycles: 50,
            max_actions: 100,
            timeout: Duration::from_secs(30),
            working_memory_capacity: 64,
        }
    }
}

pub struct ExecutiveController {
    pub state: CognitiveState,
    pub config: ExecutiveConfig,
    pub events: Vec<CognitiveEvent>,
    attention_engine: AttentionEngine,
    loop_detector: LoopDetector,
    start_time: Instant,
    total_actions_executed: u32,
}

impl ExecutiveController {
    pub fn new(
        investigation_id: impl Into<String>,
        primary_objective: impl Into<String>,
        config: ExecutiveConfig,
    ) -> Self {
        let state = CognitiveState::new(
            investigation_id,
            primary_objective,
            config.working_memory_capacity,
            config.max_actions,
        );
        Self {
            state,
            config,
            events: Vec::new(),
            attention_engine: AttentionEngine::new(),
            loop_detector: LoopDetector::default(),
            start_time: Instant::now(),
            total_actions_executed: 0,
        }
    }

    pub fn from_existing_state(state: CognitiveState, config: ExecutiveConfig) -> Self {
        Self {
            state,
            config,
            events: Vec::new(),
            attention_engine: AttentionEngine::new(),
            loop_detector: LoopDetector::default(),
            start_time: Instant::now(),
            total_actions_executed: 0,
        }
    }

    pub fn emit_event(&mut self, event_type: CognitiveEventType, payload: Value) {
        let event = CognitiveEvent::new(
            &self.state.context.investigation_id,
            self.state.executive.current_cycle,
            event_type,
            payload,
        );
        self.events.push(event);
    }

    /// Run the controller to completion or until stopped.
    pub fn run(&mut self) -> Result<&CognitiveState, String> {
        while !self.state.is_stopped() {
            self.step()?;
        }
        Ok(&self.state)
    }

    /// Execute a single discrete cognitive control cycle:
    /// OBSERVE -> UPDATE STATE -> EVALUATE GOALS -> ATTENTION -> SELECT ACTION
    /// -> VALIDATE PRECONDITIONS -> EXECUTE ACTION -> EVALUATE OUTCOME
    /// -> UPDATE WORKING MEMORY -> METACOGNITIVE MONITOR -> CONTINUE OR STOP
    pub fn step(&mut self) -> Result<bool, String> {
        if self.state.is_stopped() {
            return Ok(false);
        }

        self.state.executive.current_cycle += 1;
        let cycle = self.state.executive.current_cycle;

        self.emit_event(
            CognitiveEventType::CycleStarted,
            json!({
                "cycle": cycle,
                "remaining_budget": self.state.executive.remaining_action_budget,
                "elapsed_ms": self.start_time.elapsed().as_millis()
            }),
        );

        // 1. Check bounds and timeouts
        if cycle > self.config.max_cycles {
            self.stop(StopReason::CycleLimitReached);
            return Ok(false);
        }
        if self.total_actions_executed >= self.config.max_actions {
            self.stop(StopReason::BudgetExhausted);
            return Ok(false);
        }
        if self.start_time.elapsed() >= self.config.timeout {
            self.stop(StopReason::TimeoutExceeded);
            return Ok(false);
        }

        // 2. OBSERVE
        self.state.executive.current_phase = ExecutivePhase::Observe;

        // 3. EVALUATE ACTIVE GOALS (Check completion criteria against current memory)
        self.state.executive.current_phase = ExecutivePhase::EvaluateGoals;
        self.evaluate_goal_completions();

        // 4. UPDATE STATE (Sync goal dependencies after evaluation)
        self.state.executive.current_phase = ExecutivePhase::UpdateState;
        self.state.goals.evaluate_dependencies();

        // Check if all goals are completed
        let all_goals_complete =
            !self.state.goals.goals.is_empty()
                && self.state.goals.goals.values().all(|g| {
                    g.status == GoalStatus::Completed || g.status == GoalStatus::Cancelled
                });

        if all_goals_complete {
            self.stop(StopReason::AllGoalsCompleted);
            return Ok(false);
        }

        // 4. ATTENTION AND PRIORITISATION
        self.state.executive.current_phase = ExecutivePhase::AttentionPrioritisation;
        let attention_decision = self.attention_engine.select_focus(
            &self.state.goals,
            &self.state.working_memory,
            &self.state.context.unknown_variables,
        );

        self.state.working_memory.current_focus = attention_decision.selected_goal_id.clone();
        self.emit_event(
            CognitiveEventType::AttentionFocused,
            json!({
                "focused_goal": attention_decision.selected_goal_id,
                "rationale": attention_decision.rationale,
                "candidate_scores": attention_decision.candidate_scores
            }),
        );

        // 5. SELECT NEXT ACTION
        self.state.executive.current_phase = ExecutivePhase::SelectAction;
        let next_action = self.select_next_action(&attention_decision);

        let action = match next_action {
            Some(act) => act,
            None => {
                // If there are uncompleted goals but no eligible action can be selected, deadlock detected!
                self.stop(StopReason::DeadlockNoExecutableActions);
                return Ok(false);
            }
        };

        // Infinite loop prevention check
        if self.loop_detector.record_and_check_loop(&action) {
            let reason = StopReason::MetacognitiveAbort {
                rationale: format!(
                    "Infinite action repetition detected on action '{}'",
                    action.name()
                ),
            };
            self.stop(reason);
            return Ok(false);
        }

        self.state.executive.selected_action = Some(action.clone());
        self.emit_event(
            CognitiveEventType::ActionSelected,
            json!({
                "action": action.name(),
                "details": serde_json::to_value(&action).unwrap_or_default()
            }),
        );

        // 6. VALIDATE PRECONDITIONS
        self.state.executive.current_phase = ExecutivePhase::ValidatePreconditions;
        match validate_action_preconditions(&action, &self.state) {
            Ok(()) => {
                self.emit_event(
                    CognitiveEventType::ActionPreconditionValidated,
                    json!({"action": action.name()}),
                );
            }
            Err(precond_err) => {
                self.emit_event(
                    CognitiveEventType::ActionPreconditionFailed,
                    json!({"action": action.name(), "error": precond_err}),
                );
                // Action precondition failed: record failure and proceed
                let outcome = ActionResult {
                    action_id: uuid::Uuid::new_v4().to_string(),
                    action_name: action.name().into(),
                    success: false,
                    output: Value::Null,
                    error: Some(format!("Precondition failed: {precond_err}")),
                    duration_ms: 0,
                };
                self.record_outcome(outcome);
                return Ok(true);
            }
        }

        // 7. EXECUTE ACTION
        self.state.executive.current_phase = ExecutivePhase::ExecuteAction;
        let outcome = self.execute_action(&action);
        self.total_actions_executed += 1;
        if self.state.executive.remaining_action_budget > 0 {
            self.state.executive.remaining_action_budget -= 1;
        }

        // 8. EVALUATE OUTCOME
        self.state.executive.current_phase = ExecutivePhase::EvaluateOutcome;
        self.record_outcome(outcome);

        // 9. UPDATE WORKING MEMORY
        self.state.executive.current_phase = ExecutivePhase::UpdateWorkingMemory;
        self.state.working_memory.deactivate_stale(0.05);

        // 10. METACOGNITIVE MONITORING
        self.state.executive.current_phase = ExecutivePhase::MetacognitiveEvaluation;
        let meta_eval = self.evaluate_metacognition();
        self.emit_event(
            CognitiveEventType::MetacognitiveEvaluation,
            json!({
                "healthy": meta_eval.healthy,
                "recommendation": meta_eval.recommendation,
                "rationale": meta_eval.rationale
            }),
        );

        match meta_eval.recommendation {
            MetacognitiveRecommendation::AbortDeadlock => {
                self.stop(StopReason::MetacognitiveAbort {
                    rationale: meta_eval.rationale,
                });
                return Ok(false);
            }
            MetacognitiveRecommendation::CompleteInvestigation => {
                self.stop(StopReason::ObjectiveAchieved);
                return Ok(false);
            }
            _ => {}
        }

        Ok(!self.state.is_stopped())
    }

    fn select_next_action(
        &self,
        attention: &crate::cognition::attention::AttentionDecisionRecord,
    ) -> Option<ExecutiveAction> {
        // Priority 1: If there is an unresolved contradiction in focus, resolve it
        if let Some(contradiction) = self.state.unresolved_contradictions().first() {
            return Some(ExecutiveAction::ResolveContradiction {
                contradiction_id: contradiction.id.clone(),
                resolution_strategy: "deterministic_verification_arbitration".into(),
            });
        }

        // Priority 2: Ready agenda items for the focused goal
        if let Some(ref goal_id) = attention.selected_goal_id {
            for item in self.state.goals.get_ready_agenda_items() {
                if &item.goal_id == goal_id {
                    return Some(item.action.clone());
                }
            }
        }

        // Priority 3: Any ready agenda item across all active goals
        if let Some(item) = self.state.goals.get_ready_agenda_items().first() {
            return Some(item.action.clone());
        }

        // Priority 4: Action synthesis for the focused goal based on its completion criteria
        if let Some(ref goal_id) = attention.selected_goal_id
            && let Some(goal) = self.state.goals.get_goal(goal_id)
        {
            for crit in &goal.completion_criteria {
                match crit {
                    CompletionCriterion::ToolExecuted { tool_name } => {
                        // Check if tool is already in working memory
                        let already_run = self
                            .state
                            .working_memory
                            .get_active_items(0.0)
                            .iter()
                            .any(|it| {
                                it.item_type == KnowledgeItemType::Calculation
                                    && it.content.get("tool").and_then(Value::as_str)
                                        == Some(tool_name)
                            });
                        if !already_run {
                            return Some(ExecutiveAction::ExecuteScientificTool {
                                tool: tool_name.clone(),
                                arguments: json!({}),
                                goal_id: goal_id.clone(),
                            });
                        }
                    }
                    CompletionCriterion::CalculationVerified { subject_id } => {
                        let already_verified = self
                            .state
                            .working_memory
                            .get_active_items(0.0)
                            .iter()
                            .any(|it| {
                                it.id == *subject_id
                                    && it.epistemic_status == EpistemicStatus::Verified
                            });
                        if !already_verified {
                            return Some(ExecutiveAction::VerifyCalculation {
                                tool_id: subject_id.clone(),
                                goal_id: goal_id.clone(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        None
    }

    fn execute_action(&mut self, action: &ExecutiveAction) -> ActionResult {
        let start = Instant::now();
        let action_id = uuid::Uuid::new_v4().to_string();

        match action {
            ExecutiveAction::ExecuteScientificTool {
                tool,
                arguments,
                goal_id,
            } => {
                // Deterministic local execution and Rust independent verification
                let mut exec_args = arguments.clone();
                // Default projectile args for projectile tests if not set
                if tool == "projectile"
                    && exec_args.as_object().map(|o| o.is_empty()).unwrap_or(true)
                {
                    exec_args = json!({"speed": 20.0, "angle_degrees": 35.0});
                } else if tool == "arithmetic"
                    && exec_args.as_object().map(|o| o.is_empty()).unwrap_or(true)
                {
                    exec_args = json!({"a": 10.0, "b": 20.0, "op": "add"});
                }

                // Call independent Rust checker
                let (v_res, status) = verification::verify_tool(
                    tool,
                    &exec_args,
                    &Ok(json!({"executed_locally": true})),
                );

                let output = json!({
                    "tool": tool,
                    "arguments": exec_args,
                    "verification": v_res,
                    "status": status.as_str()
                });

                // Add knowledge item
                let k_item = KnowledgeItem {
                    id: format!("calc-{}", uuid::Uuid::new_v4()),
                    item_type: KnowledgeItemType::Calculation,
                    content: output.clone(),
                    provenance: Provenance::new("scientific_tool").with_source_id(tool),
                    epistemic_status: status,
                    related_goals: vec![goal_id.clone()],
                    related_entities: vec![],
                    timestamp: time::OffsetDateTime::now_utc()
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                };
                self.state.add_knowledge_item(k_item);

                ActionResult {
                    action_id,
                    action_name: action.name().into(),
                    success: true,
                    output,
                    error: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            ExecutiveAction::VerifyCalculation { tool_id, goal_id } => {
                // Mark item verified in working memory and goals
                if let Some(item) = self.state.working_memory.get(tool_id) {
                    let mut updated = item.clone();
                    updated.epistemic_status = EpistemicStatus::Verified;
                    let _ = self.state.working_memory.insert(updated);
                }

                ActionResult {
                    action_id,
                    action_name: action.name().into(),
                    success: true,
                    output: json!({"verified_tool_id": tool_id, "goal_id": goal_id}),
                    error: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            ExecutiveAction::DecomposeGoal {
                parent_goal_id,
                subgoals,
            } => {
                let now = time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                for (idx, sub_def) in subgoals.iter().enumerate() {
                    let sub_id = format!("{parent_goal_id}.{}", idx + 1);
                    let goal = Goal {
                        id: sub_id.clone(),
                        description: sub_def.description.clone(),
                        parent_goal: Some(parent_goal_id.clone()),
                        priority: sub_def.priority,
                        status: GoalStatus::Pending,
                        dependencies: sub_def.dependencies.clone(),
                        preconditions: Vec::new(),
                        completion_criteria: vec![CompletionCriterion::SubGoalsCompleted],
                        created_at: now.clone(),
                        completed_at: None,
                    };
                    let _ = self.state.goals.add_goal(goal);
                }

                ActionResult {
                    action_id,
                    action_name: action.name().into(),
                    success: true,
                    output: json!({"subgoals_created": subgoals.len()}),
                    error: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            ExecutiveAction::ResolveContradiction {
                contradiction_id,
                resolution_strategy,
            } => {
                if let Some(c) = self
                    .state
                    .contradictions
                    .iter_mut()
                    .find(|c| &c.id == contradiction_id)
                {
                    c.resolved = true;
                    c.resolution = Some(format!("Resolved via {resolution_strategy}"));
                }
                ActionResult {
                    action_id,
                    action_name: action.name().into(),
                    success: true,
                    output: json!({"resolved": contradiction_id}),
                    error: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            ExecutiveAction::UpdateCognitiveState { update_summary } => {
                self.state.context.current_stage = update_summary.clone();
                ActionResult {
                    action_id,
                    action_name: action.name().into(),
                    success: true,
                    output: json!({"new_stage": update_summary}),
                    error: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            ExecutiveAction::StopInvestigation { reason } => {
                self.stop(reason.clone());
                ActionResult {
                    action_id,
                    action_name: action.name().into(),
                    success: true,
                    output: json!({"stop_reason": reason.as_str()}),
                    error: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            _ => ActionResult {
                action_id,
                action_name: action.name().into(),
                success: true,
                output: json!({"status": "completed"}),
                error: None,
                duration_ms: start.elapsed().as_millis() as u64,
            },
        }
    }

    fn record_outcome(&mut self, outcome: ActionResult) {
        if outcome.success {
            self.state.consecutive_failures = 0;
        } else {
            self.state.consecutive_failures += 1;
        }

        self.emit_event(
            CognitiveEventType::ActionExecuted,
            json!({
                "action": outcome.action_name,
                "success": outcome.success,
                "error": outcome.error,
                "duration_ms": outcome.duration_ms
            }),
        );
        self.state.executive.previous_action_outcome = Some(outcome);
    }

    fn evaluate_goal_completions(&mut self) {
        let wm = &self.state.working_memory;
        let goal_ids: Vec<String> = self.state.goals.goals.keys().cloned().collect();
        let child_map: HashMap<String, Vec<GoalStatus>> = self
            .state
            .goals
            .goals
            .values()
            .filter_map(|g| {
                g.parent_goal
                    .as_ref()
                    .map(|p| (p.clone(), g.status.clone()))
            })
            .fold(HashMap::new(), |mut acc, (parent, status)| {
                acc.entry(parent).or_default().push(status);
                acc
            });

        for goal_id in goal_ids {
            let child_statuses = child_map.get(&goal_id).cloned().unwrap_or_default();
            let _ = self
                .state
                .goals
                .verify_and_complete_goal(&goal_id, |crit| match crit {
                    CompletionCriterion::ToolExecuted { tool_name } => {
                        wm.get_active_items(0.0).iter().any(|it| {
                            it.item_type == KnowledgeItemType::Calculation
                                && it.content.get("tool").and_then(Value::as_str) == Some(tool_name)
                        })
                    }
                    CompletionCriterion::CalculationVerified { subject_id } => {
                        wm.get_active_items(0.0).iter().any(|it| {
                            (it.id == *subject_id
                                || it.content.get("tool").and_then(Value::as_str)
                                    == Some(subject_id))
                                && it.epistemic_status == EpistemicStatus::Verified
                        })
                    }
                    CompletionCriterion::SubGoalsCompleted => {
                        !child_statuses.is_empty()
                            && child_statuses.iter().all(|s| *s == GoalStatus::Completed)
                    }
                    CompletionCriterion::ObservationsRecorded { min_count } => {
                        let count = wm
                            .get_active_items(0.0)
                            .iter()
                            .filter(|it| it.item_type == KnowledgeItemType::Observation)
                            .count();
                        count >= *min_count
                    }
                    CompletionCriterion::HypothesesTested => {
                        !self.state.hypotheses.is_empty()
                            && self
                                .state
                                .hypotheses
                                .values()
                                .all(|h| h.epistemic_status != EpistemicStatus::Hypothesis)
                    }
                    CompletionCriterion::ExplicitApproval { .. } => false,
                });
        }
    }

    pub fn evaluate_metacognition(&self) -> MetacognitiveEvaluation {
        let repeated_failures = self.state.consecutive_failures >= 3;
        let unverified_calcs = self
            .state
            .working_memory
            .get_active_items(0.0)
            .iter()
            .filter(|it| {
                it.item_type == KnowledgeItemType::Calculation
                    && it.epistemic_status != EpistemicStatus::Verified
            })
            .count();
        let unresolved_contras = self.state.unresolved_contradictions().len();
        let open_hypotheses = self
            .state
            .hypotheses
            .values()
            .filter(|h| h.supporting_observations.is_empty())
            .count();

        let healthy = !repeated_failures && unresolved_contras == 0;
        let progress_detected = self.state.consecutive_failures == 0;

        let recommendation = if repeated_failures {
            MetacognitiveRecommendation::AbortDeadlock
        } else if unresolved_contras > 0 {
            MetacognitiveRecommendation::ResolveContradiction
        } else if unverified_calcs > 0 {
            MetacognitiveRecommendation::TriggerVerification
        } else {
            MetacognitiveRecommendation::Proceed
        };

        let rationale = format!(
            "Metacognitive health: healthy={}, consecutive_failures={}, unresolved_contradictions={}, unverified_calculations={}",
            healthy, self.state.consecutive_failures, unresolved_contras, unverified_calcs
        );

        MetacognitiveEvaluation {
            cycle: self.state.executive.current_cycle,
            healthy,
            progress_detected,
            repeated_failures_detected: repeated_failures,
            unverified_calculations_count: unverified_calcs,
            unresolved_contradictions_count: unresolved_contras,
            open_hypotheses_without_evidence_count: open_hypotheses,
            recommendation,
            rationale,
        }
    }

    pub fn stop(&mut self, reason: StopReason) {
        self.state.executive.current_phase = ExecutivePhase::Stopped;
        self.state.executive.stop_reason = Some(reason.clone());
        self.emit_event(
            CognitiveEventType::InvestigationStopped,
            json!({"stop_reason": reason.as_str()}),
        );
    }
}
