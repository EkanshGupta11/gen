use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    cognition::{
        actions::{ActionExecutionStatus, ActionResult, ExecutiveAction, StopReason},
        attention::AttentionEngine,
        events::{CognitiveEvent, CognitiveEventType},
        executor::resolve_specialist_for_tool_execution,
        goals::{CompletionCriterion, Goal, GoalStatus},
        policy::{LoopDetector, validate_action_preconditions},
        state::{
            CognitiveState, ContradictionResolution, ExecutivePhase, KnowledgeItem, Provenance,
            ResolutionOutcome, check_contradiction_comparability,
        },
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
    pub max_concurrency: usize,
}

impl Default for ExecutiveConfig {
    fn default() -> Self {
        Self {
            max_cycles: 50,
            max_actions: 100,
            timeout: Duration::from_secs(30),
            working_memory_capacity: 64,
            max_concurrency: 1,
        }
    }
}

pub struct ExecutiveController {
    pub state: CognitiveState,
    pub config: ExecutiveConfig,
    pub events: Vec<CognitiveEvent>,
    pub attention_engine: AttentionEngine,
    pub loop_detector: LoopDetector,
    start_time: Instant,
    pub total_actions_executed: u32,
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
        let total_actions_executed = state.executive.total_actions_executed;
        let loop_detector =
            LoopDetector::from_history(state.executive.loop_detection_history.clone(), 6);
        Self {
            state,
            config,
            events: Vec::new(),
            attention_engine: AttentionEngine::new(),
            loop_detector,
            start_time: Instant::now(),
            total_actions_executed,
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

    /// Prepare next action step:
    /// Check bounds -> Observe -> EvaluateGoals -> UpdateState -> Attention -> SelectAction
    /// Authoritative Executive Control lifecycle for single actions or concurrent batches:
    /// CycleStart -> Observe -> EvaluateGoals -> UpdateState -> CheckTerminalGoals
    /// -> AttentionPrioritisation -> SelectAction(s) -> LoopDetection -> ActionSelected
    /// -> ValidatePreconditions -> Return actions ready for execution.
    pub fn step_prepare_batch(&mut self, max_batch: usize) -> Result<Vec<ExecutiveAction>, String> {
        if self.state.is_stopped() {
            return Ok(Vec::new());
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
            return Ok(Vec::new());
        }
        if self.total_actions_executed >= self.config.max_actions
            || self.state.executive.remaining_action_budget == 0
        {
            self.stop(StopReason::BudgetExhausted);
            return Ok(Vec::new());
        }
        if self.start_time.elapsed() >= self.config.timeout {
            self.stop(StopReason::TimeoutExceeded);
            return Ok(Vec::new());
        }

        // 2. OBSERVE
        self.state.executive.current_phase = ExecutivePhase::Observe;

        // 3. EVALUATE ACTIVE GOALS (Check completion criteria against current memory)
        self.state.executive.current_phase = ExecutivePhase::EvaluateGoals;
        self.evaluate_goal_completions();

        // 4. UPDATE STATE (Sync goal dependencies after evaluation)
        self.state.executive.current_phase = ExecutivePhase::UpdateState;
        self.state.goals.evaluate_dependencies();

        // 5. HONEST TERMINAL GOAL EVALUATION (Cancelled goals never count as completed)
        let has_goals = !self.state.goals.goals.is_empty();
        let all_goals_completed = has_goals
            && self
                .state
                .goals
                .goals
                .values()
                .all(|g| g.status == GoalStatus::Completed);
        let all_goals_cancelled = has_goals
            && self
                .state
                .goals
                .goals
                .values()
                .all(|g| g.status == GoalStatus::Cancelled);
        let all_goals_terminal = has_goals
            && self
                .state
                .goals
                .goals
                .values()
                .all(|g| g.status.is_terminal());
        let any_goal_cancelled = self
            .state
            .goals
            .goals
            .values()
            .any(|g| g.status == GoalStatus::Cancelled);
        let any_goal_failed = self
            .state
            .goals
            .goals
            .values()
            .any(|g| g.status == GoalStatus::Failed);

        // Explicit Primary Synthesis Objective Evaluation
        let synthesis_goal_opt = self.state.goals.goals.values().find(|g| {
            g.completion_criteria
                .iter()
                .any(|c| matches!(c, CompletionCriterion::SynthesisCompleted))
        });

        if let Some(synthesis_goal) = synthesis_goal_opt {
            if synthesis_goal.status == GoalStatus::Completed {
                if any_goal_cancelled {
                    self.stop(StopReason::UserCancelled);
                } else {
                    self.stop(StopReason::ObjectiveAchieved);
                }
                return Ok(Vec::new());
            } else if synthesis_goal.status == GoalStatus::Cancelled {
                self.stop(StopReason::UserCancelled);
                return Ok(Vec::new());
            } else if synthesis_goal.status == GoalStatus::Failed {
                // If the primary synthesis objective has failed, the entire investigation cannot achieve its objective
                self.stop(StopReason::DeadlockNoExecutableActions);
                return Ok(Vec::new());
            }
        }

        if all_goals_completed {
            self.stop(StopReason::AllGoalsCompleted);
            return Ok(Vec::new());
        }

        if all_goals_cancelled {
            self.stop(StopReason::UserCancelled);
            return Ok(Vec::new());
        }

        if all_goals_terminal {
            if any_goal_cancelled {
                self.stop(StopReason::UserCancelled);
                return Ok(Vec::new());
            } else if any_goal_failed {
                self.stop(StopReason::DeadlockNoExecutableActions);
                return Ok(Vec::new());
            }
        }

        // 6. ATTENTION AND PRIORITISATION
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

        // 7. SELECT ACTIONS (Strictly bounded by max_batch, max_concurrency, and remaining action budget)
        self.state.executive.current_phase = ExecutivePhase::SelectAction;
        let budget_cap = self.state.executive.remaining_action_budget as usize;
        let effective_limit = max_batch.min(self.config.max_concurrency).min(budget_cap);

        if effective_limit == 0 {
            self.stop(StopReason::BudgetExhausted);
            return Ok(Vec::new());
        }

        let mut candidate_actions = Vec::new();
        if let Some(contradiction) = self.state.unresolved_contradictions().first() {
            // Priority: Resolve contradiction sequentially before other actions
            candidate_actions.push(ExecutiveAction::ResolveContradiction {
                contradiction_id: contradiction.id.clone(),
                resolution_strategy: "deterministic_verification_arbitration".into(),
            });
        } else if effective_limit > 1 {
            let concurrent_actions = self.get_concurrent_ready_actions(effective_limit);
            if concurrent_actions.len() > 1 {
                candidate_actions = concurrent_actions;
            } else if let Some(act) = self.select_next_action(&attention_decision) {
                candidate_actions.push(act);
            }
        } else if let Some(act) = self.select_next_action(&attention_decision) {
            candidate_actions.push(act);
        }

        if candidate_actions.is_empty() {
            // Uncompleted goals remain but no eligible action can be selected: deadlock
            self.stop(StopReason::DeadlockNoExecutableActions);
            return Ok(Vec::new());
        }

        // 8. LOOP DETECTION FOR EVERY ACTION IN BATCH
        for action in &candidate_actions {
            if self.loop_detector.record_and_check_loop(action) {
                let reason = StopReason::MetacognitiveAbort {
                    rationale: format!(
                        "Infinite action repetition detected on action '{}'",
                        action.name()
                    ),
                };
                self.stop(reason);
                return Ok(Vec::new());
            }
        }

        // 9. RECORD SELECTION AND EMIT EVENTS
        self.state.executive.selected_action = candidate_actions.first().cloned();
        for action in &candidate_actions {
            self.emit_event(
                CognitiveEventType::ActionSelected,
                json!({
                    "action": action.name(),
                    "details": serde_json::to_value(action).unwrap_or_default()
                }),
            );
        }

        // 10. VALIDATE PRECONDITIONS FOR ALL ACTIONS IN BATCH
        self.state.executive.current_phase = ExecutivePhase::ValidatePreconditions;
        let mut ready_batch = Vec::new();
        for action in candidate_actions {
            match validate_action_preconditions(&action, &self.state) {
                Ok(()) => {
                    self.emit_event(
                        CognitiveEventType::ActionPreconditionValidated,
                        json!({"action": action.name()}),
                    );
                    ready_batch.push(action);
                }
                Err(precond_err) => {
                    self.emit_event(
                        CognitiveEventType::ActionPreconditionFailed,
                        json!({"action": action.name(), "error": precond_err}),
                    );
                    let outcome = ActionResult::failed(
                        uuid::Uuid::new_v4().to_string(),
                        action.name(),
                        format!("Precondition failed: {precond_err}"),
                        0,
                    );
                    self.step_commit(&action, outcome)?;
                }
            }
        }

        Ok(ready_batch)
    }

    /// Single-action preparation backward-compatible wrapper.
    pub fn step_prepare(&mut self) -> Result<Option<ExecutiveAction>, String> {
        let mut batch = self.step_prepare_batch(1)?;
        Ok(batch.pop())
    }

    /// Returns up to `limit` ready agenda actions for distinct active goals that do not depend on each other.
    pub fn get_concurrent_ready_actions(&self, limit: usize) -> Vec<ExecutiveAction> {
        if limit <= 1 || self.state.is_stopped() {
            return Vec::new();
        }
        let ready_items = self.state.goals.get_ready_agenda_items();
        if ready_items.len() <= 1 {
            return Vec::new();
        }

        let mut actions = Vec::new();
        let mut seen_goals: std::collections::HashSet<String> = std::collections::HashSet::new();

        for item in ready_items {
            if actions.len() >= limit {
                break;
            }
            if seen_goals.contains(&item.goal_id) {
                continue;
            }
            // Ensure this goal does not depend on any other goal currently in the batch
            // AND no goal currently in the batch depends on this goal
            if let Some(goal) = self.state.goals.get_goal(&item.goal_id) {
                let conflicts_with_batch = goal
                    .dependencies
                    .iter()
                    .any(|dep| seen_goals.contains(dep.as_str()))
                    || seen_goals.iter().any(|existing_id| {
                        self.state
                            .goals
                            .get_goal(existing_id)
                            .map(|g| g.dependencies.contains(&item.goal_id))
                            .unwrap_or(false)
                    });
                if !conflicts_with_batch
                    && validate_action_preconditions(&item.action, &self.state).is_ok()
                {
                    seen_goals.insert(item.goal_id.clone());
                    actions.push(item.action.clone());
                }
            }
        }

        if actions.len() > 1 {
            actions
        } else {
            Vec::new()
        }
    }

    /// Commit the outcome of an executed action:
    /// EvaluateOutcome -> UpdateWorkingMemory -> MetacognitiveEvaluation -> ContinueOrStop.
    pub fn step_commit(
        &mut self,
        action: &ExecutiveAction,
        outcome: ActionResult,
    ) -> Result<bool, String> {
        let _ = action;
        self.total_actions_executed += 1;
        self.state.executive.total_actions_executed = self.total_actions_executed;
        if self.state.executive.remaining_action_budget > 0 {
            self.state.executive.remaining_action_budget -= 1;
        }
        self.state.executive.loop_detection_history = self.loop_detector.history();

        // 8. EVALUATE OUTCOME
        self.state.executive.current_phase = ExecutivePhase::EvaluateOutcome;
        self.record_outcome(outcome);

        // Update agenda item status for completed action
        for item in &mut self.state.goals.agenda {
            if &item.action == action
                && (item.status == crate::cognition::goals::AgendaStatus::Ready
                    || item.status == crate::cognition::goals::AgendaStatus::Active)
            {
                item.status = if self.state.consecutive_failures == 0 {
                    crate::cognition::goals::AgendaStatus::Completed
                } else {
                    crate::cognition::goals::AgendaStatus::Failed
                };
                break;
            }
        }

        // 9. UPDATE WORKING MEMORY
        self.state.executive.current_phase = ExecutivePhase::UpdateWorkingMemory;
        self.state.working_memory.deactivate_stale(0.05);

        // Immediately evaluate completions and sync dependencies so state reflects action outcome
        self.evaluate_goal_completions();
        self.state.goals.evaluate_dependencies();

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

    /// Execute a single discrete cognitive control cycle synchronously:
    pub fn step(&mut self) -> Result<bool, String> {
        if self.state.is_stopped() {
            return Ok(false);
        }

        match self.step_prepare()? {
            Some(action) => {
                self.state.executive.current_phase = ExecutivePhase::ExecuteAction;
                let outcome = self.execute_action(&action);
                self.step_commit(&action, outcome)
            }
            None => Ok(!self.state.is_stopped()),
        }
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
            && goal.status == GoalStatus::Active
        {
            for crit in &goal.completion_criteria {
                match crit {
                    CompletionCriterion::ToolExecuted { tool_name } => {
                        // Check if tool is already in working memory for this specific goal
                        let already_run = self
                            .state
                            .working_memory
                            .get_active_items(0.0)
                            .iter()
                            .any(|it| {
                                it.related_goals.contains(goal_id)
                                    && it.item_type == KnowledgeItemType::Calculation
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
                                it.related_goals.contains(goal_id)
                                    && (it.id == *subject_id
                                        || it.content.get("tool").and_then(Value::as_str)
                                            == Some(subject_id))
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

    pub fn execute_action(&mut self, action: &ExecutiveAction) -> ActionResult {
        let start = Instant::now();
        let action_id = uuid::Uuid::new_v4().to_string();

        match action {
            ExecutiveAction::ExecuteScientificTool {
                tool,
                arguments,
                goal_id,
            } => {
                let mut exec_args = arguments.clone();
                let mut computed_val = json!({});

                if tool == "projectile" {
                    if exec_args.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                        exec_args = json!({"speed": 20.0, "angle_degrees": 35.0});
                    }
                    let speed = exec_args
                        .get("speed")
                        .and_then(Value::as_f64)
                        .unwrap_or(20.0);
                    let angle_deg = exec_args
                        .get("angle_degrees")
                        .and_then(Value::as_f64)
                        .unwrap_or(35.0);
                    let angle_rad = angle_deg.to_radians();
                    let g = 9.80665;
                    let range = (speed * speed * (2.0 * angle_rad).sin()) / g;
                    let flight_time = (2.0 * speed * angle_rad.sin()) / g;
                    let max_height = (speed * speed * angle_rad.sin().powi(2)) / (2.0 * g);
                    computed_val = json!({
                        "range_m": range,
                        "flight_time_s": flight_time,
                        "max_height_m": max_height,
                        "assumptions": ["vacuum", "level launch and landing", "constant gravity"],
                        "units": "SI"
                    });
                } else if tool == "arithmetic" {
                    if exec_args.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                        exec_args = json!({"a": 10.0, "b": 20.0, "op": "add"});
                    }
                    let a = exec_args.get("a").and_then(Value::as_f64).unwrap_or(0.0);
                    let b = exec_args.get("b").and_then(Value::as_f64).unwrap_or(0.0);
                    let op = exec_args.get("op").and_then(Value::as_str).unwrap_or("add");
                    let val = match op {
                        "add" => a + b,
                        "sub" => a - b,
                        "multiply" | "mul" => a * b,
                        "div" if b != 0.0 => a / b,
                        _ => a + b,
                    };
                    computed_val = json!({
                        "value": val,
                        "operation": op,
                        "assumptions": ["exact integer or floating-point arithmetic"],
                        "units": "dimensionless"
                    });
                }

                // Call independent Rust checker with actual computed numbers
                let (v_res, status) =
                    verification::verify_tool(tool, &exec_args, &Ok(computed_val.clone()));

                let specialist = resolve_specialist_for_tool_execution(goal_id, tool, &self.state);
                let output = json!({
                    "tool": tool,
                    "specialist": specialist.as_str(),
                    "arguments": exec_args,
                    "output": computed_val,
                    "verification": v_res,
                    "status": status.as_str()
                });

                // Add knowledge item
                let k_item_id = format!("calc-{}", uuid::Uuid::new_v4());
                let k_item = KnowledgeItem {
                    id: k_item_id.clone(),
                    item_type: KnowledgeItemType::Calculation,
                    content: output.clone(),
                    provenance: Provenance::new("scientific_tool")
                        .with_source_id(specialist.as_str()),
                    epistemic_status: status.clone(),
                    related_goals: vec![goal_id.clone()],
                    related_entities: vec![],
                    timestamp: time::OffsetDateTime::now_utc()
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                };
                self.state.add_knowledge_item(k_item);

                if status == EpistemicStatus::Contradicted {
                    self.state.record_contradiction(
                        format!("contra-{}", uuid::Uuid::new_v4()),
                        format!("Tool '{tool}' contradicted independent verification"),
                        vec![k_item_id],
                    );
                    ActionResult::contradicted(
                        action_id,
                        action.name(),
                        output,
                        "Calculation contradicted independent verification",
                        start.elapsed().as_millis() as u64,
                    )
                } else {
                    ActionResult::succeeded(
                        action_id,
                        action.name(),
                        output,
                        start.elapsed().as_millis() as u64,
                    )
                }
            }
            ExecutiveAction::VerifyCalculation { tool_id, goal_id } => {
                let duration_ms = start.elapsed().as_millis() as u64;

                let item_opt = self
                    .state
                    .knowledge_items
                    .get(tool_id)
                    .or_else(|| {
                        self.state.knowledge_items.values().find(|k| {
                            k.content.get("tool").and_then(Value::as_str) == Some(tool_id)
                        })
                    })
                    .cloned();

                let item = match item_opt {
                    Some(it) => it,
                    None => {
                        return ActionResult::failed(
                            action_id,
                            action.name(),
                            format!("Calculation '{tool_id}' not found in state"),
                            duration_ms,
                        );
                    }
                };

                let tool_name = item
                    .content
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let arguments = item
                    .content
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let worker_output = item.content.get("output").cloned();

                let worker_res = match worker_output {
                    Some(ref val) if !val.is_null() && val.get("error").is_none() => {
                        Ok(val.clone())
                    }
                    Some(val) => Err(val
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("Worker output error")
                        .to_string()),
                    None => Err("No output recorded".to_string()),
                };

                let (v_res, epistemic_status) =
                    verification::verify_tool(&tool_name, &arguments, &worker_res);

                if let Some(k_item) = self.state.knowledge_items.get_mut(&item.id) {
                    k_item.epistemic_status = epistemic_status.clone();
                }
                if let Some(wm_item) = self.state.working_memory.get_mut(&item.id) {
                    wm_item.epistemic_status = epistemic_status.clone();
                }

                let output = json!({
                    "verified_tool_id": item.id,
                    "goal_id": goal_id,
                    "verification": v_res,
                    "epistemic_status": epistemic_status.as_str()
                });

                if epistemic_status == EpistemicStatus::Contradicted {
                    self.state.record_contradiction(
                        format!("contra-{}", uuid::Uuid::new_v4()),
                        format!("Tool '{}' contradicted verification", item.id),
                        vec![item.id.clone()],
                    );
                    ActionResult::contradicted(
                        action_id,
                        action.name(),
                        output,
                        "Calculation contradicted independent verification",
                        duration_ms,
                    )
                } else if epistemic_status == EpistemicStatus::Verified {
                    ActionResult::succeeded(action_id, action.name(), output, duration_ms)
                } else {
                    ActionResult::new(
                        action_id,
                        action.name(),
                        ActionExecutionStatus::Uncertain,
                        output,
                        Some("Independent verification failed or checker unavailable".into()),
                        duration_ms,
                    )
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

                ActionResult::succeeded(
                    action_id,
                    action.name(),
                    json!({"subgoals_created": subgoals.len()}),
                    start.elapsed().as_millis() as u64,
                )
            }
            ExecutiveAction::ResolveContradiction {
                contradiction_id,
                resolution_strategy,
            } => {
                let duration_ms = start.elapsed().as_millis() as u64;
                let c_opt = self
                    .state
                    .contradictions
                    .iter()
                    .position(|c| &c.id == contradiction_id);

                let idx = match c_opt {
                    Some(i) => i,
                    None => {
                        return ActionResult::failed(
                            action_id,
                            action.name(),
                            format!("Contradiction '{contradiction_id}' not found"),
                            duration_ms,
                        );
                    }
                };

                let conflicting_ids = self.state.contradictions[idx].conflicting_items.clone();
                let mut verified_evidence = Vec::new();
                let mut contradicted_items = Vec::new();

                for item_id in &conflicting_ids {
                    if let Some(item) = self.state.knowledge_items.get(item_id) {
                        if item.epistemic_status == EpistemicStatus::Verified {
                            verified_evidence.push(item_id.clone());
                        } else if item.epistemic_status == EpistemicStatus::Contradicted {
                            contradicted_items.push(item_id.clone());
                        }
                    }
                }

                let now = time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();

                if verified_evidence.is_empty() {
                    let res = ContradictionResolution {
                        contradiction_id: contradiction_id.clone(),
                        conflicting_items: conflicting_ids,
                        proposed_resolution: format!(
                            "Cannot resolve contradiction via {resolution_strategy} without verified evidence. Requires human review."
                        ),
                        supporting_evidence_refs: vec![],
                        verification_method: "none".into(),
                        outcome: ResolutionOutcome::RequiresHumanReview,
                        timestamp: now,
                    };
                    self.state.contradictions[idx].resolved = false;
                    self.state.contradictions[idx].resolution = Some(res.clone());

                    return ActionResult::new(
                        action_id,
                        action.name(),
                        ActionExecutionStatus::Uncertain,
                        serde_json::to_value(&res).unwrap_or_default(),
                        Some("Cannot resolve contradiction without verified evidence".into()),
                        duration_ms,
                    );
                }

                if verified_evidence.len() > 1 {
                    let res = ContradictionResolution {
                        contradiction_id: contradiction_id.clone(),
                        conflicting_items: conflicting_ids,
                        proposed_resolution: format!(
                            "Multiple conflicting verified evidence items found ({:?}). Cannot automatically resolve contradiction via {resolution_strategy}. Requires human review.",
                            verified_evidence
                        ),
                        supporting_evidence_refs: verified_evidence,
                        verification_method: "multiple_conflicting_verified".into(),
                        outcome: ResolutionOutcome::RequiresHumanReview,
                        timestamp: now,
                    };
                    self.state.contradictions[idx].resolved = false;
                    self.state.contradictions[idx].resolution = Some(res.clone());

                    return ActionResult::new(
                        action_id,
                        action.name(),
                        ActionExecutionStatus::Uncertain,
                        serde_json::to_value(&res).unwrap_or_default(),
                        Some("Multiple conflicting verified items require human review".into()),
                        duration_ms,
                    );
                }

                // Exactly one verified evidence item: check comparability with all other conflicting items
                let verified_id = &verified_evidence[0];
                let verified_item = self.state.knowledge_items.get(verified_id).cloned();

                let mut comparability_errors = Vec::new();
                if let Some(v_item) = &verified_item {
                    for other_id in &conflicting_ids {
                        if other_id == verified_id {
                            continue;
                        }
                        if let Some(other_item) = self.state.knowledge_items.get(other_id) {
                            if let Err(reason) =
                                check_contradiction_comparability(v_item, other_item)
                            {
                                comparability_errors.push(format!("Item '{other_id}': {reason}"));
                            }
                        } else {
                            comparability_errors.push(format!(
                                "Conflicting item '{other_id}' missing from knowledge base"
                            ));
                        }
                    }
                } else {
                    comparability_errors
                        .push("Verified evidence item missing from knowledge base".into());
                }

                if !comparability_errors.is_empty() {
                    let res = ContradictionResolution {
                        contradiction_id: contradiction_id.clone(),
                        conflicting_items: conflicting_ids,
                        proposed_resolution: format!(
                            "Verified evidence is not directly comparable to conflicting items: {}. Requires human review.",
                            comparability_errors.join("; ")
                        ),
                        supporting_evidence_refs: verified_evidence,
                        verification_method: "comparability_check_failed".into(),
                        outcome: ResolutionOutcome::RequiresHumanReview,
                        timestamp: now,
                    };
                    self.state.contradictions[idx].resolved = false;
                    self.state.contradictions[idx].resolution = Some(res.clone());

                    return ActionResult::new(
                        action_id,
                        action.name(),
                        ActionExecutionStatus::Uncertain,
                        serde_json::to_value(&res).unwrap_or_default(),
                        Some(format!(
                            "Incomparable evidence: {}",
                            comparability_errors.join("; ")
                        )),
                        duration_ms,
                    );
                }

                let res = ContradictionResolution {
                    contradiction_id: contradiction_id.clone(),
                    conflicting_items: conflicting_ids,
                    proposed_resolution: format!(
                        "Resolved via {resolution_strategy}: verified evidence {:?} upheld, contradicted {:?} rejected",
                        verified_evidence, contradicted_items
                    ),
                    supporting_evidence_refs: verified_evidence,
                    verification_method: "rust_independent_verification".into(),
                    outcome: ResolutionOutcome::Resolved,
                    timestamp: now,
                };
                self.state.contradictions[idx].resolved = true;
                self.state.contradictions[idx].resolution = Some(res.clone());

                ActionResult::succeeded(
                    action_id,
                    action.name(),
                    serde_json::to_value(&res).unwrap_or_default(),
                    duration_ms,
                )
            }
            ExecutiveAction::UpdateCognitiveState { update_summary } => {
                self.state.context.current_stage = update_summary.clone();
                ActionResult::succeeded(
                    action_id,
                    action.name(),
                    json!({"new_stage": update_summary}),
                    start.elapsed().as_millis() as u64,
                )
            }
            ExecutiveAction::StopInvestigation { reason } => {
                self.stop(reason.clone());
                ActionResult::succeeded(
                    action_id,
                    action.name(),
                    json!({"stop_reason": reason.as_str()}),
                    start.elapsed().as_millis() as u64,
                )
            }
            ExecutiveAction::CompareHypotheses { hypothesis_ids } => {
                let mut comparisons = Vec::new();
                for hid in hypothesis_ids {
                    if let Some(h) = self.state.hypotheses.get(hid) {
                        comparisons.push(json!({
                            "id": h.id,
                            "statement": h.statement,
                            "supporting_count": h.supporting_observations.len(),
                            "contradicting_count": h.contradicting_observations.len(),
                        }));
                    }
                }
                ActionResult::succeeded(
                    action_id,
                    action.name(),
                    json!({"comparisons": comparisons}),
                    start.elapsed().as_millis() as u64,
                )
            }
            unimplemented => {
                // Critical Defect E fix: An unimplemented action must NEVER report success!
                ActionResult::unsupported(
                    action_id,
                    unimplemented.name(),
                    format!(
                        "Action '{}' is not implemented in synchronous executor",
                        unimplemented.name()
                    ),
                    start.elapsed().as_millis() as u64,
                )
            }
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
                "status": outcome.status.as_str(),
                "success": outcome.success,
                "error": outcome.error,
                "duration_ms": outcome.duration_ms
            }),
        );
        self.state.executive.previous_action_outcome = Some(outcome);
    }

    pub fn evaluate_goal_completions(&mut self) {
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
                        self.state.knowledge_items.values().any(|k| {
                            k.related_goals.contains(&goal_id)
                                && k.item_type == KnowledgeItemType::Calculation
                                && (k.content.get("tool").and_then(Value::as_str)
                                    == Some(tool_name)
                                    || k.provenance.source_id.as_deref() == Some(tool_name))
                        }) || wm.get_active_items(0.0).iter().any(|it| {
                            it.related_goals.contains(&goal_id)
                                && it.item_type == KnowledgeItemType::Calculation
                                && it.content.get("tool").and_then(Value::as_str) == Some(tool_name)
                        })
                    }
                    CompletionCriterion::CalculationVerified { subject_id } => {
                        self.state.knowledge_items.values().any(|k| {
                            k.related_goals.contains(&goal_id)
                                && k.item_type == KnowledgeItemType::Calculation
                                && (k.id == *subject_id
                                    || k.content.get("tool").and_then(Value::as_str)
                                        == Some(subject_id)
                                    || k.provenance.source_id.as_deref() == Some(subject_id))
                                && k.epistemic_status == EpistemicStatus::Verified
                        }) || wm.get_active_items(0.0).iter().any(|it| {
                            it.related_goals.contains(&goal_id)
                                && (it.id == *subject_id
                                    || it.content.get("tool").and_then(Value::as_str)
                                        == Some(subject_id))
                                && it.epistemic_status == EpistemicStatus::Verified
                        })
                    }
                    CompletionCriterion::SpecialistReportReceived { specialist } => {
                        self.state.knowledge_items.values().any(|k| {
                            k.related_goals.contains(&goal_id)
                                && (k.provenance.source_id.as_deref() == Some(specialist.as_str())
                                    || k.content.get("specialist").and_then(Value::as_str)
                                        == Some(specialist.as_str()))
                        })
                    }
                    CompletionCriterion::SynthesisCompleted => {
                        self.state
                            .knowledge_items
                            .values()
                            .any(|k| k.provenance.source_type == "synthesis")
                            || self.state.context.current_stage == "completed"
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
        if self.state.executive.stop_reason.is_some() {
            return;
        }
        self.state.executive.current_phase = ExecutivePhase::Stopped;
        self.state.executive.stop_reason = Some(reason.clone());
        self.emit_event(
            CognitiveEventType::InvestigationStopped,
            json!({"stop_reason": reason.as_str()}),
        );
    }
}
