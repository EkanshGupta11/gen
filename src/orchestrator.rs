use std::{ffi::OsString, path::PathBuf, sync::Arc, time::Duration};

#[cfg(test)]
use serde_json::Value;
use serde_json::json;
use uuid::Uuid;

use crate::{
    contracts::*,
    provider::{self, GenerationRequest, GenerationStage, ModelProvider, RequestBudget},
    store::Store,
};

pub struct Orchestrator {
    pub store: Store,
    pub provider: Arc<dyn ModelProvider>,
    pub python: OsString,
    pub worker_script: PathBuf,
    pub worker_timeout: Duration,
    pub max_concurrency: usize,
}

impl Orchestrator {
    pub async fn investigate(&self, question: String) -> Result<Investigation, String> {
        let now = timestamp();
        let mut record = Investigation {
            contract_version: "1.0".into(),
            id: Uuid::new_v4().to_string(),
            question: question.clone(),
            created_at: now.clone(),
            updated_at: now,
            status: InvestigationStatus::Planning,
            mode: if self.provider.is_mock() {
                "mock_offline"
            } else {
                "hosted_model"
            }
            .into(),
            plan: None,
            specialist_reports: vec![],
            tools: vec![],
            verification: vec![],
            synthesis: None,
            model: ModelMetadata {
                provider: self.provider.settings().model_provider,
                model: self.provider.settings().model_name,
                request_count: 0,
                http_attempt_count: 0,
                usage: None,
            },
            sources: vec![],
            error: None,
            cognitive_state: None,
        };
        let initial_cognitive_state = crate::cognition::CognitiveState::new(
            record.id.clone(),
            question.clone(),
            crate::cognition::DEFAULT_WORKING_MEMORY_CAPACITY,
            50,
        );
        record.cognitive_state = Some(initial_cognitive_state);
        self.store.save(&record).await.map_err(|e| e.to_string())?;

        let budget = Arc::new(RequestBudget::new(
            self.provider.settings().max_tokens_per_request,
        ));
        let run: Result<Investigation, String> = async {
            let plan: InvestigationPlan = provider::generate_structured(
                self.provider.as_ref(),
                GenerationRequest {
                    stage: GenerationStage::Planning,
                    specialist: None,
                    system_instruction: "Decompose the research question into a bounded structured plan. Return only the requested JSON schema. No executable instructions. Use concise objective, entities, variables, constraints, expected_output, subproblems [{id,specialist,task,depends_on,required_tools}], assumptions, uncertainties. Select only relevant specialists; at most five subproblems. Do not expose hidden reasoning.".into(),
                    input: json!({"question": question}),
                },
                budget.as_ref(),
            )
            .await
            .map_err(|e| e.to_string())?;

            validate_plan(&plan)?;
            record.plan = Some(plan.clone());
            record.status = InvestigationStatus::Executing;
            record.updated_at = timestamp();

            let mut cs = crate::cognition::CognitiveState::new(
                record.id.clone(),
                record.question.clone(),
                crate::cognition::DEFAULT_WORKING_MEMORY_CAPACITY,
                50,
            );
            cs.context.relevant_entities = plan.entities.clone();
            cs.context.known_variables = plan.known_variables.clone();
            cs.context.unknown_variables = plan.unknown_variables.clone();
            cs.context.constraints = plan.constraints.clone();
            cs.context.current_stage = "executing_subproblems".into();

            for sub in &plan.subproblems {
                let mut criteria = vec![
                    crate::cognition::CompletionCriterion::SpecialistReportReceived {
                        specialist: sub.specialist.as_str().to_string(),
                    },
                ];
                for tool in &sub.required_tools {
                    criteria.push(crate::cognition::CompletionCriterion::ToolExecuted {
                        tool_name: tool.clone(),
                    });
                    criteria.push(crate::cognition::CompletionCriterion::CalculationVerified {
                        subject_id: tool.clone(),
                    });
                }

                let goal = crate::cognition::Goal {
                    id: sub.id.clone(),
                    description: sub.task.clone(),
                    parent_goal: None,
                    priority: 50,
                    status: if sub.depends_on.is_empty() {
                        crate::cognition::GoalStatus::Active
                    } else {
                        crate::cognition::GoalStatus::Pending
                    },
                    dependencies: sub.depends_on.clone(),
                    preconditions: Vec::new(),
                    completion_criteria: criteria,
                    created_at: timestamp(),
                    completed_at: None,
                };
                cs.goals.add_goal(goal)?;

                cs.goals.add_agenda_item(crate::cognition::AgendaItem {
                    id: format!("agenda-{}", sub.id),
                    goal_id: sub.id.clone(),
                    action: crate::cognition::ExecutiveAction::RequestSpecialistAnalysis {
                        specialist: sub.specialist.clone(),
                        task: sub.task.clone(),
                        goal_id: sub.id.clone(),
                    },
                    status: if sub.depends_on.is_empty() {
                        crate::cognition::AgendaStatus::Ready
                    } else {
                        crate::cognition::AgendaStatus::Blocked
                    },
                    created_at: timestamp(),
                });
            }

            // Synthesis goal
            let all_sub_ids: Vec<String> =
                plan.subproblems.iter().map(|s| s.id.clone()).collect();
            let synth_goal = crate::cognition::Goal {
                id: "synthesis-goal".into(),
                description: "Synthesize findings and establish validated research conclusion".into(),
                parent_goal: None,
                priority: 20,
                status: crate::cognition::GoalStatus::Pending,
                dependencies: all_sub_ids.clone(),
                preconditions: Vec::new(),
                completion_criteria: vec![crate::cognition::CompletionCriterion::SynthesisCompleted],
                created_at: timestamp(),
                completed_at: None,
            };
            cs.goals.add_goal(synth_goal)?;

            cs.goals.add_agenda_item(crate::cognition::AgendaItem {
                id: "agenda-synthesis".into(),
                goal_id: "synthesis-goal".into(),
                action: crate::cognition::ExecutiveAction::SynthesizeFindings {
                    goal_ids: all_sub_ids,
                },
                status: crate::cognition::AgendaStatus::Blocked,
                created_at: timestamp(),
            });

            record.cognitive_state = Some(cs.clone());
            self.store.save(&record).await.map_err(|e| e.to_string())?;

            let executor = crate::cognition::AsyncActionExecutor::new(
                self.python.clone(),
                self.worker_script.clone(),
                self.worker_timeout,
                self.provider.clone(),
                budget.clone(),
            );

            let config = crate::cognition::ExecutiveConfig {
                max_cycles: 50,
                max_actions: 100,
                timeout: Duration::from_secs(45),
                working_memory_capacity: crate::cognition::DEFAULT_WORKING_MEMORY_CAPACITY,
                max_concurrency: self.max_concurrency,
            };

            let mut runtime = crate::cognition::CognitiveRuntime::from_existing_state(
                cs,
                config,
                executor,
                self.store.clone(),
            );

            let runtime_run_result = runtime.run().await;
            let final_state = runtime.state().clone();

            // Reconstruct specialist reports in subproblem order
            for sub in &plan.subproblems {
                if let Some(item) = final_state.knowledge_items.values().find(|k| {
                    k.provenance.source_type == "specialist_report"
                        && k.related_goals.contains(&sub.id)
                }) && let Ok(rep) = serde_json::from_value::<AgentReport>(item.content.clone()) {
                    record.specialist_reports.push(rep);
                    continue;
                }

                // Check if a dependency failed
                let failed_dep = sub.depends_on.iter().find(|dep_id| {
                    final_state
                        .goals
                        .get_goal(dep_id)
                        .map(|g| {
                            g.status == crate::cognition::GoalStatus::Failed
                                || g.status == crate::cognition::GoalStatus::Cancelled
                        })
                        .unwrap_or(false)
                });

                if let Some(dep_name) = failed_dep {
                    record.specialist_reports.push(AgentReport {
                        specialist: sub.specialist.clone(),
                        status: AgentExecutionStatus::Failed,
                        analysis: format!("Dependency '{}' failed, preventing execution.", dep_name),
                        claims: vec![],
                        tool_calls: vec![],
                        assumptions: vec![],
                        uncertainties: vec![format!("Dependency failure: {dep_name}")],
                        evidence_requirements: vec![],
                        duration_ms: 0,
                    });
                } else if let Some(goal) = final_state.goals.get_goal(&sub.id)
                    && (goal.status == crate::cognition::GoalStatus::Blocked
                        || goal.status == crate::cognition::GoalStatus::Failed)
                {
                    record.specialist_reports.push(AgentReport {
                        specialist: sub.specialist.clone(),
                        status: AgentExecutionStatus::Failed,
                        analysis: "Unsatisfied subproblem dependency prevented execution.".into(),
                        claims: vec![],
                        tool_calls: vec![],
                        assumptions: vec![],
                        uncertainties: vec!["Dependency resolution failure".into()],
                        evidence_requirements: vec![],
                        duration_ms: 0,
                    });
                }
            }

            // Extract tools and verifications from knowledge items in subproblem plan order
            for sub in &plan.subproblems {
                let mut sub_tools: Vec<_> = final_state
                    .knowledge_items
                    .values()
                    .filter(|k| {
                        k.item_type == crate::cognition::KnowledgeItemType::Calculation
                            && k.related_goals.contains(&sub.id)
                    })
                    .collect();
                sub_tools.sort_by_key(|k| &k.timestamp);
                for item in sub_tools {
                    if let Ok(tool) = serde_json::from_value::<ToolExecution>(item.content.clone()) {
                        if let Some(v_val) = item.content.get("verification")
                            && let Ok(v_res) = serde_json::from_value::<VerificationResult>(v_val.clone())
                            && !record.verification.iter().any(|v| v.id == v_res.id)
                        {
                            record.verification.push(v_res);
                        }
                        if !record.tools.iter().any(|t| t.id == tool.id) {
                            record.tools.push(tool);
                        }
                    }
                }
            }

            // Extract any remaining tools not attached to a specific subproblem
            let mut remaining_tools: Vec<_> = final_state
                .knowledge_items
                .values()
                .filter(|k| k.item_type == crate::cognition::KnowledgeItemType::Calculation)
                .collect();
            remaining_tools.sort_by_key(|k| &k.timestamp);
            for item in remaining_tools {
                if let Ok(tool) = serde_json::from_value::<ToolExecution>(item.content.clone()) {
                    if let Some(v_val) = item.content.get("verification")
                        && let Ok(v_res) = serde_json::from_value::<VerificationResult>(v_val.clone())
                        && !record.verification.iter().any(|v| v.id == v_res.id)
                    {
                        record.verification.push(v_res);
                    }
                    if !record.tools.iter().any(|t| t.id == tool.id) {
                        record.tools.push(tool);
                    }
                }
            }

            // Extract synthesis
            if let Some(item) = final_state.knowledge_items.values().find(|k| {
                k.provenance.source_type == "synthesis"
            }) && let Ok(syn) = serde_json::from_value::<Synthesis>(item.content.clone()) {
                record.synthesis = Some(syn);
            }

            record.cognitive_state = Some(final_state.clone());
            record.model.request_count = budget.model_call_count();
            record.model.http_attempt_count = budget.http_attempt_count();
            record.model.usage = budget.usage();
            record.updated_at = timestamp();

            // 1. Runtime Error Propagation: A runtime failure must never result in Completed!
            if let Err(runtime_err) = runtime_run_result {
                record.status = InvestigationStatus::Failed;
                record.error = Some(runtime_err.clone());
                if let Err(save_err) = self.store.save(&record).await {
                    return Err(format!(
                        "{runtime_err}; additionally failed to persist failure record: {save_err}"
                    ));
                }
                return Err(runtime_err);
            }

            // 2. Honest Terminal Status: Map every StopReason to an honest investigation outcome
            let has_goals = !final_state.goals.goals.is_empty();
            let all_goals_genuinely_completed = has_goals
                && final_state
                    .goals
                    .goals
                    .values()
                    .all(|g| g.status == crate::cognition::GoalStatus::Completed);
            let any_goal_cancelled = final_state
                .goals
                .goals
                .values()
                .any(|g| g.status == crate::cognition::GoalStatus::Cancelled);
            let any_goal_failed = final_state
                .goals
                .goals
                .values()
                .any(|g| g.status == crate::cognition::GoalStatus::Failed);

            let (status, err_opt) = map_stop_reason_to_investigation_outcome(
                final_state.executive.stop_reason.as_ref(),
                all_goals_genuinely_completed,
                any_goal_cancelled,
                any_goal_failed,
            );
            record.status = status;
            if let Some(err) = err_opt {
                record.error = Some(err);
            }
            if matches!(
                final_state.executive.stop_reason,
                Some(crate::cognition::StopReason::MetacognitiveAbort { .. })
            ) {
                let err_msg = record
                    .error
                    .clone()
                    .unwrap_or_else(|| "Metacognitive abort".into());
                if let Err(save_err) = self.store.save(&record).await {
                    return Err(format!(
                        "{err_msg}; additionally failed to persist abort record: {save_err}"
                    ));
                }
                return Err(err_msg);
            }

            self.store.save(&record).await.map_err(|e| e.to_string())?;
            Ok(record.clone())
        }
        .await;

        match run {
            Ok(record) => Ok(record),
            Err(error) => {
                record.status = InvestigationStatus::Failed;
                record.error = Some(error.clone());
                record.updated_at = timestamp();
                record.model.request_count = budget.model_call_count();
                record.model.http_attempt_count = budget.http_attempt_count();
                if let Some(cs) = record.cognitive_state.as_mut() {
                    cs.context.current_stage = "failed".into();
                    cs.executive.current_phase = crate::cognition::ExecutivePhase::Stopped;
                    cs.executive.stop_reason =
                        Some(crate::cognition::StopReason::MetacognitiveAbort {
                            rationale: error.clone(),
                        });
                }
                if let Err(save_err) = self.store.save(&record).await {
                    Err(format!(
                        "{error}; additionally failed to persist failure record: {save_err}"
                    ))
                } else {
                    Err(error)
                }
            }
        }
    }
}

fn timestamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn validate_plan(plan: &InvestigationPlan) -> Result<(), String> {
    if plan.subproblems.is_empty() || plan.subproblems.len() > 5 {
        return Err("Planner returned an invalid number of subproblems".into());
    }
    let ids: std::collections::HashSet<_> = plan
        .subproblems
        .iter()
        .map(|problem| problem.id.as_str())
        .collect();
    if ids.len() != plan.subproblems.len()
        || plan
            .subproblems
            .iter()
            .any(|problem| problem.id.trim().is_empty() || problem.task.trim().is_empty())
    {
        return Err("Planner returned duplicate or empty subproblem identifiers/tasks".into());
    }
    for problem in &plan.subproblems {
        if problem
            .depends_on
            .iter()
            .any(|dependency| dependency == &problem.id || !ids.contains(dependency.as_str()))
        {
            return Err(format!(
                "Planner returned an invalid dependency for {}",
                problem.id
            ));
        }
        if problem
            .required_tools
            .iter()
            .any(|tool| !problem.specialist.allowed_tools().contains(&tool.as_str()))
        {
            return Err(format!(
                "Planner requested a tool outside the {} allowlist",
                problem.specialist.as_str()
            ));
        }
    }
    let mut visited = std::collections::HashSet::new();
    let mut pending = std::collections::HashSet::new();
    fn visit<'a>(
        id: &'a str,
        plan: &'a InvestigationPlan,
        visited: &mut std::collections::HashSet<&'a str>,
        pending: &mut std::collections::HashSet<&'a str>,
    ) -> Result<(), String> {
        if pending.contains(id) {
            return Err("Planner returned cyclic subproblem dependencies".into());
        }
        if visited.contains(id) {
            return Ok(());
        }
        pending.insert(id);
        let problem = plan
            .subproblems
            .iter()
            .find(|problem| problem.id == id)
            .expect("validated plan id");
        for dependency in &problem.depends_on {
            visit(dependency, plan, visited, pending)?;
        }
        pending.remove(id);
        visited.insert(id);
        Ok(())
    }
    for problem in &plan.subproblems {
        visit(&problem.id, plan, &mut visited, &mut pending)?;
    }
    Ok(())
}

/// Maps an executive StopReason to an honest InvestigationStatus and error message.
/// Ensures budget exhaustion, timeout, cycle limit, deadlock, and cancellation never silently become Completed.
pub fn map_stop_reason_to_investigation_outcome(
    stop_reason: Option<&crate::cognition::StopReason>,
    all_goals_genuinely_completed: bool,
    any_goal_cancelled: bool,
    any_goal_failed: bool,
) -> (InvestigationStatus, Option<String>) {
    match stop_reason {
        Some(crate::cognition::StopReason::ObjectiveAchieved) => {
            if any_goal_cancelled {
                (
                    InvestigationStatus::Interrupted,
                    Some("Investigation partially cancelled by user".into()),
                )
            } else {
                (InvestigationStatus::Completed, None)
            }
        }
        Some(crate::cognition::StopReason::AllGoalsCompleted) => {
            if any_goal_cancelled {
                (
                    InvestigationStatus::Interrupted,
                    Some("Investigation partially cancelled by user".into()),
                )
            } else if any_goal_failed {
                (
                    InvestigationStatus::Failed,
                    Some("One or more goals failed during investigation".into()),
                )
            } else if all_goals_genuinely_completed {
                (InvestigationStatus::Completed, None)
            } else {
                (
                    InvestigationStatus::Interrupted,
                    Some("Investigation stopped before all goals were completed".into()),
                )
            }
        }
        Some(crate::cognition::StopReason::BudgetExhausted) => (
            InvestigationStatus::Interrupted,
            Some("Action budget exhausted before completing all goals".into()),
        ),
        Some(crate::cognition::StopReason::CycleLimitReached) => (
            InvestigationStatus::Interrupted,
            Some("Executive cycle limit reached before completing all goals".into()),
        ),
        Some(crate::cognition::StopReason::TimeoutExceeded) => (
            InvestigationStatus::Interrupted,
            Some("Executive timeout exceeded before completing all goals".into()),
        ),
        Some(crate::cognition::StopReason::DeadlockNoExecutableActions) => (
            InvestigationStatus::Failed,
            Some("Deadlock detected: no executable actions remain for uncompleted goals".into()),
        ),
        Some(crate::cognition::StopReason::MetacognitiveAbort { rationale }) => {
            (InvestigationStatus::Failed, Some(rationale.clone()))
        }
        Some(crate::cognition::StopReason::UserCancelled) => (
            InvestigationStatus::Interrupted,
            Some("Investigation cancelled by user".into()),
        ),
        None => {
            if all_goals_genuinely_completed && !any_goal_cancelled && !any_goal_failed {
                (InvestigationStatus::Completed, None)
            } else if any_goal_cancelled {
                (
                    InvestigationStatus::Interrupted,
                    Some("Investigation cancelled or partially cancelled".into()),
                )
            } else if any_goal_failed {
                (
                    InvestigationStatus::Failed,
                    Some("One or more goals failed during investigation".into()),
                )
            } else {
                (
                    InvestigationStatus::Interrupted,
                    Some("Investigation stopped unexpectedly before completing all goals".into()),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MockProvider;

    fn get_python_exe() -> &'static str {
        if cfg!(windows) {
            if std::path::Path::new(".venv-cpython313/Scripts/python.exe").exists() {
                ".venv-cpython313/Scripts/python.exe"
            } else {
                ".venv/bin/python.exe"
            }
        } else {
            "python3"
        }
    }

    #[tokio::test]
    async fn offline_investigation_runs_plan_specialist_tool_synthesis_and_persists() {
        let directory = tempfile::tempdir().unwrap();
        let (store, _) =
            Store::initialize(directory.path().join("investigations.sqlite3")).unwrap();
        let python = get_python_exe();
        let orchestration = Orchestrator {
            store,
            provider: Arc::new(MockProvider { max_tokens: 1200 }),
            python: python.into(),
            worker_script: PathBuf::from("engine/app.py"),
            worker_timeout: Duration::from_secs(5),
            max_concurrency: 3,
        };
        let record = orchestration
            .investigate(
                "For a projectile launched at speed 20 m/s and angle 35 degrees, estimate ideal range."
                    .into(),
            )
            .await
            .unwrap();

        assert_eq!(record.status, InvestigationStatus::Completed);
        assert_eq!(record.mode, "mock_offline");
        assert!(
            record
                .specialist_reports
                .iter()
                .any(|report| report.specialist == Specialist::Physics
                    && report.status == AgentExecutionStatus::Mock)
        );
        assert_eq!(record.tools.len(), 1);
        let tool_exec = &record.tools[0];
        let range = tool_exec.output.as_ref().unwrap()["range_m"]
            .as_f64()
            .unwrap();
        assert!((range - 38.33).abs() < 0.02);

        // Verification validation:
        // Priority 1 verification must mark the deterministic calculation as Verified!
        assert_eq!(tool_exec.epistemic_status, EpistemicStatus::Verified);
        assert!(tool_exec.verification_id.is_some());
        assert_eq!(record.verification.len(), 1);
        let v = &record.verification[0];
        assert_eq!(v.status, EpistemicStatus::Verified);
        assert!(v.independently_checked);
        assert_eq!(
            v.checker.as_deref(),
            Some("rust_independent_projectile_checker")
        );
        assert!(v.absolute_error.unwrap() <= 1e-4);

        assert!(record.synthesis.is_some());
        assert_eq!(
            orchestration
                .store
                .get(record.id.clone())
                .await
                .unwrap()
                .unwrap()
                .status,
            "completed"
        );
        assert_eq!(record.model.request_count, 4);
    }

    #[tokio::test]
    async fn serial_specialist_execution_mode() {
        let directory = tempfile::tempdir().unwrap();
        let (store, _) =
            Store::initialize(directory.path().join("serial_investigations.sqlite3")).unwrap();
        let python = get_python_exe();
        let orchestration = Orchestrator {
            store,
            provider: Arc::new(MockProvider { max_tokens: 1200 }),
            python: python.into(),
            worker_script: PathBuf::from("engine/app.py"),
            worker_timeout: Duration::from_secs(5),
            max_concurrency: 1, // Explicit serial mode
        };
        let record = orchestration
            .investigate(
                "For a projectile launched at speed 20 m/s and angle 35 degrees, estimate ideal range."
                    .into(),
            )
            .await
            .unwrap();

        assert_eq!(record.status, InvestigationStatus::Completed);
        assert_eq!(record.tools.len(), 1);
        assert_eq!(record.tools[0].epistemic_status, EpistemicStatus::Verified);
    }

    #[tokio::test]
    async fn specialist_failure_isolation_test() {
        // A custom provider where a specific specialist stage fails,
        // proving failure isolation allows synthesis and remaining specialists to complete.
        struct FailingSpecialistProvider;

        #[async_trait::async_trait]
        impl ModelProvider for FailingSpecialistProvider {
            async fn complete(
                &self,
                req: GenerationRequest,
                _budget: &RequestBudget,
            ) -> Result<provider::ModelCompletion, provider::ModelError> {
                match req.stage {
                    GenerationStage::Planning => {
                        let plan = json!({
                            "objective": "Test failure isolation",
                            "entities": [],
                            "known_variables": [],
                            "unknown_variables": [],
                            "constraints": [],
                            "expected_output": "Report",
                            "subproblems": [
                                {
                                    "id": "sub-1",
                                    "specialist": Specialist::Chemistry,
                                    "task": "Will fail",
                                    "depends_on": [],
                                    "required_tools": []
                                },
                                {
                                    "id": "sub-2",
                                    "specialist": Specialist::Mathematics,
                                    "task": "Will succeed",
                                    "depends_on": [],
                                    "required_tools": ["arithmetic"]
                                }
                            ],
                            "assumptions": [],
                            "uncertainties": []
                        });
                        Ok(provider::ModelCompletion {
                            content: serde_json::to_string(&plan).unwrap(),
                            usage: None,
                        })
                    }
                    GenerationStage::Specialist
                        if req.specialist == Some(Specialist::Chemistry) =>
                    {
                        Err(provider::ModelError::Http {
                            status: 500,
                            message: "Injected specialist failure".into(),
                        })
                    }
                    GenerationStage::Specialist => {
                        let out = json!({
                            "analysis": "Mathematics completed normally",
                            "claims": [],
                            "tool_calls": [{
                                "tool": "arithmetic",
                                "arguments": {"a": 10, "b": 32, "op": "add"},
                                "rationale": "Add numbers"
                            }],
                            "assumptions": [],
                            "uncertainties": [],
                            "evidence_requirements": []
                        });
                        Ok(provider::ModelCompletion {
                            content: serde_json::to_string(&out).unwrap(),
                            usage: None,
                        })
                    }
                    GenerationStage::Synthesis => {
                        let syn = json!({
                            "executive_summary": "Completed despite Chemistry failure",
                            "key_findings": [],
                            "disagreements": [],
                            "assumptions": [],
                            "uncertainties": ["Chemistry specialist encountered 500"],
                            "next_steps": []
                        });
                        Ok(provider::ModelCompletion {
                            content: serde_json::to_string(&syn).unwrap(),
                            usage: None,
                        })
                    }
                }
            }

            fn settings(&self) -> ProviderSettings {
                ProviderSettings {
                    model_provider: "failing_test".into(),
                    model_name: Some("test-fail".into()),
                    memory_provider: "sqlite",
                    credentials_configured: true,
                    max_tokens_per_request: 1000,
                }
            }

            fn is_mock(&self) -> bool {
                false
            }
        }

        let directory = tempfile::tempdir().unwrap();
        let (store, _) = Store::initialize(directory.path().join("fail_isolated.sqlite3")).unwrap();
        let orchestration = Orchestrator {
            store,
            provider: Arc::new(FailingSpecialistProvider),
            python: get_python_exe().into(),
            worker_script: PathBuf::from("engine/app.py"),
            worker_timeout: Duration::from_secs(5),
            max_concurrency: 2,
        };

        let record = orchestration
            .investigate("Compare chemistry and mathematics".into())
            .await
            .unwrap();

        // The investigation should succeed overall, isolating the failure
        assert_eq!(record.status, InvestigationStatus::Completed);
        assert_eq!(record.specialist_reports.len(), 2);

        let chem_report = record
            .specialist_reports
            .iter()
            .find(|r| r.specialist == Specialist::Chemistry)
            .unwrap();
        assert_eq!(chem_report.status, AgentExecutionStatus::Failed);
        assert!(chem_report.analysis.contains("Injected specialist failure"));

        let math_report = record
            .specialist_reports
            .iter()
            .find(|r| r.specialist == Specialist::Mathematics)
            .unwrap();
        assert_eq!(math_report.status, AgentExecutionStatus::Completed);

        // Arithmetic tool was verified
        assert_eq!(record.tools.len(), 1);
        assert_eq!(record.tools[0].epistemic_status, EpistemicStatus::Verified);
    }

    #[tokio::test]
    async fn regression_specialist_deduplication_preserves_distinct_subproblems_same_role() {
        struct DuplicateRoleProvider;

        #[async_trait::async_trait]
        impl ModelProvider for DuplicateRoleProvider {
            async fn complete(
                &self,
                req: GenerationRequest,
                _budget: &RequestBudget,
            ) -> Result<provider::ModelCompletion, provider::ModelError> {
                match req.stage {
                    GenerationStage::Planning => {
                        let plan = json!({
                            "objective": "Test duplicate role preservation",
                            "entities": [],
                            "known_variables": [],
                            "unknown_variables": [],
                            "constraints": [],
                            "expected_output": "Two math results",
                            "subproblems": [
                                {
                                    "id": "math-1",
                                    "specialist": Specialist::Mathematics,
                                    "task": "Compute addition: 15 + 25",
                                    "depends_on": [],
                                    "required_tools": ["arithmetic"]
                                },
                                {
                                    "id": "math-2",
                                    "specialist": Specialist::Mathematics,
                                    "task": "Compute multiplication: 6 * 7",
                                    "depends_on": [],
                                    "required_tools": ["arithmetic"]
                                }
                            ],
                            "assumptions": [],
                            "uncertainties": []
                        });
                        Ok(provider::ModelCompletion {
                            content: serde_json::to_string(&plan).unwrap(),
                            usage: None,
                        })
                    }
                    GenerationStage::Specialist => {
                        let task_str = req.input.get("task").and_then(Value::as_str).unwrap_or("");
                        let tool_call = if task_str.contains("addition") {
                            json!({
                                "tool": "arithmetic",
                                "arguments": {"a": 15, "b": 25, "op": "add"},
                                "rationale": "Add 15 and 25"
                            })
                        } else {
                            json!({
                                "tool": "arithmetic",
                                "arguments": {"a": 6, "b": 7, "op": "multiply"},
                                "rationale": "Multiply 6 and 7"
                            })
                        };
                        let out = json!({
                            "analysis": format!("Executed task: {task_str}"),
                            "claims": [],
                            "tool_calls": [tool_call],
                            "assumptions": [],
                            "uncertainties": [],
                            "evidence_requirements": []
                        });
                        Ok(provider::ModelCompletion {
                            content: serde_json::to_string(&out).unwrap(),
                            usage: None,
                        })
                    }
                    GenerationStage::Synthesis => {
                        let syn = json!({
                            "executive_summary": "Both mathematics tasks completed.",
                            "key_findings": [],
                            "disagreements": [],
                            "assumptions": [],
                            "uncertainties": [],
                            "next_steps": []
                        });
                        Ok(provider::ModelCompletion {
                            content: serde_json::to_string(&syn).unwrap(),
                            usage: None,
                        })
                    }
                }
            }

            fn settings(&self) -> ProviderSettings {
                ProviderSettings {
                    model_provider: "test_dedup".into(),
                    model_name: Some("test-dedup".into()),
                    memory_provider: "sqlite",
                    credentials_configured: true,
                    max_tokens_per_request: 1000,
                }
            }

            fn is_mock(&self) -> bool {
                false
            }
        }

        let directory = tempfile::tempdir().unwrap();
        let (store, _) = Store::initialize(directory.path().join("dedup_test.sqlite3")).unwrap();
        let orchestration = Orchestrator {
            store,
            provider: Arc::new(DuplicateRoleProvider),
            python: get_python_exe().into(),
            worker_script: PathBuf::from("engine/app.py"),
            worker_timeout: Duration::from_secs(5),
            max_concurrency: 2,
        };

        let record = orchestration
            .investigate("Two distinct mathematics subproblems".into())
            .await
            .unwrap();

        assert_eq!(record.status, InvestigationStatus::Completed);
        // DEFECT A REGRESSION CHECK: Both subproblems must be preserved and executed!
        assert_eq!(
            record.specialist_reports.len(),
            2,
            "Both subproblems must execute even if they share the same specialist role"
        );
        assert_eq!(record.tools.len(), 2);
        let results: Vec<f64> = record
            .tools
            .iter()
            .map(|t| t.output.as_ref().unwrap()["value"].as_f64().unwrap())
            .collect();
        assert!(results.contains(&40.0));
        assert!(results.contains(&42.0));
    }

    #[tokio::test]
    async fn regression_dependency_context_propagation_to_downstream_tasks() {
        struct DependencyPropagationProvider;

        #[async_trait::async_trait]
        impl ModelProvider for DependencyPropagationProvider {
            async fn complete(
                &self,
                req: GenerationRequest,
                _budget: &RequestBudget,
            ) -> Result<provider::ModelCompletion, provider::ModelError> {
                match req.stage {
                    GenerationStage::Planning => {
                        let plan = json!({
                            "objective": "Test dependency propagation",
                            "entities": [],
                            "known_variables": [],
                            "unknown_variables": [],
                            "constraints": [],
                            "expected_output": "Chained calculation",
                            "subproblems": [
                                {
                                    "id": "step-1",
                                    "specialist": Specialist::Mathematics,
                                    "task": "Base calculation",
                                    "depends_on": [],
                                    "required_tools": ["arithmetic"]
                                },
                                {
                                    "id": "step-2",
                                    "specialist": Specialist::Mathematics,
                                    "task": "Downstream calculation requiring step-1 results",
                                    "depends_on": ["step-1"],
                                    "required_tools": ["arithmetic"]
                                }
                            ],
                            "assumptions": [],
                            "uncertainties": []
                        });
                        Ok(provider::ModelCompletion {
                            content: serde_json::to_string(&plan).unwrap(),
                            usage: None,
                        })
                    }
                    GenerationStage::Specialist => {
                        let task_str = req.input.get("task").and_then(Value::as_str).unwrap_or("");
                        let tool_results = req.input.get("tool_results").and_then(Value::as_array);

                        if task_str.contains("Downstream") {
                            // DEFECT B REGRESSION CHECK:
                            // Verify that step-1 tool output was successfully propagated to step-2!
                            assert!(
                                tool_results.is_some() && !tool_results.unwrap().is_empty(),
                                "Downstream task step-2 must receive prior tool results from step-1!"
                            );
                            let prior_result = tool_results.unwrap()[0]
                                .get("output")
                                .and_then(|o| o.get("value"))
                                .and_then(Value::as_f64)
                                .unwrap();
                            assert_eq!(prior_result, 30.0);

                            let tool_call = json!({
                                "tool": "arithmetic",
                                "arguments": {"a": prior_result, "b": 2.0, "op": "multiply"},
                                "rationale": "Double the step-1 result"
                            });
                            let out = json!({
                                "analysis": "Step 2 executed with step 1 context",
                                "claims": [],
                                "tool_calls": [tool_call],
                                "assumptions": [],
                                "uncertainties": [],
                                "evidence_requirements": []
                            });
                            Ok(provider::ModelCompletion {
                                content: serde_json::to_string(&out).unwrap(),
                                usage: None,
                            })
                        } else {
                            let tool_call = json!({
                                "tool": "arithmetic",
                                "arguments": {"a": 10.0, "b": 20.0, "op": "add"},
                                "rationale": "Initial sum"
                            });
                            let out = json!({
                                "analysis": "Step 1 executed",
                                "claims": [],
                                "tool_calls": [tool_call],
                                "assumptions": [],
                                "uncertainties": [],
                                "evidence_requirements": []
                            });
                            Ok(provider::ModelCompletion {
                                content: serde_json::to_string(&out).unwrap(),
                                usage: None,
                            })
                        }
                    }
                    GenerationStage::Synthesis => {
                        let syn = json!({
                            "executive_summary": "Dependency chain completed successfully.",
                            "key_findings": [],
                            "disagreements": [],
                            "assumptions": [],
                            "uncertainties": [],
                            "next_steps": []
                        });
                        Ok(provider::ModelCompletion {
                            content: serde_json::to_string(&syn).unwrap(),
                            usage: None,
                        })
                    }
                }
            }

            fn settings(&self) -> ProviderSettings {
                ProviderSettings {
                    model_provider: "test_dep_prop".into(),
                    model_name: Some("test-dep-prop".into()),
                    memory_provider: "sqlite",
                    credentials_configured: true,
                    max_tokens_per_request: 1000,
                }
            }

            fn is_mock(&self) -> bool {
                false
            }
        }

        // Test with max_concurrency: 2
        let directory = tempfile::tempdir().unwrap();
        let (store, _) = Store::initialize(directory.path().join("prop_test.sqlite3")).unwrap();
        let orchestration = Orchestrator {
            store,
            provider: Arc::new(DependencyPropagationProvider),
            python: get_python_exe().into(),
            worker_script: PathBuf::from("engine/app.py"),
            worker_timeout: Duration::from_secs(5),
            max_concurrency: 2,
        };

        let record = orchestration
            .investigate("Chained calculations with dependency context".into())
            .await
            .unwrap();

        assert_eq!(record.status, InvestigationStatus::Completed);
        assert_eq!(record.tools.len(), 2);
        assert_eq!(
            record.tools[1].output.as_ref().unwrap()["value"]
                .as_f64()
                .unwrap(),
            60.0
        );
    }

    #[tokio::test]
    async fn regression_failed_dependency_is_surfaced_to_downstream_tasks() {
        struct FailingDependencyProvider;

        #[async_trait::async_trait]
        impl ModelProvider for FailingDependencyProvider {
            async fn complete(
                &self,
                req: GenerationRequest,
                _budget: &RequestBudget,
            ) -> Result<provider::ModelCompletion, provider::ModelError> {
                match req.stage {
                    GenerationStage::Planning => {
                        let plan = json!({
                            "objective": "Test failed dependency propagation",
                            "entities": [],
                            "known_variables": [],
                            "unknown_variables": [],
                            "constraints": [],
                            "expected_output": "Fail downstream",
                            "subproblems": [
                                {
                                    "id": "step-fail",
                                    "specialist": Specialist::Physics,
                                    "task": "Fails at model level",
                                    "depends_on": [],
                                    "required_tools": []
                                },
                                {
                                    "id": "step-dependent",
                                    "specialist": Specialist::Mathematics,
                                    "task": "Should not execute because step-fail failed",
                                    "depends_on": ["step-fail"],
                                    "required_tools": ["arithmetic"]
                                }
                            ],
                            "assumptions": [],
                            "uncertainties": []
                        });
                        Ok(provider::ModelCompletion {
                            content: serde_json::to_string(&plan).unwrap(),
                            usage: None,
                        })
                    }
                    GenerationStage::Specialist if req.specialist == Some(Specialist::Physics) => {
                        Err(provider::ModelError::Http {
                            status: 500,
                            message: "Physics step exploded".into(),
                        })
                    }
                    GenerationStage::Specialist => {
                        panic!("step-dependent should never be called when dependency fails!");
                    }
                    GenerationStage::Synthesis => {
                        let syn = json!({
                            "executive_summary": "Surfaced dependency failure cleanly.",
                            "key_findings": [],
                            "disagreements": [],
                            "assumptions": [],
                            "uncertainties": ["Step fail exploded"],
                            "next_steps": []
                        });
                        Ok(provider::ModelCompletion {
                            content: serde_json::to_string(&syn).unwrap(),
                            usage: None,
                        })
                    }
                }
            }

            fn settings(&self) -> ProviderSettings {
                ProviderSettings {
                    model_provider: "test_fail_dep".into(),
                    model_name: Some("test-fail-dep".into()),
                    memory_provider: "sqlite",
                    credentials_configured: true,
                    max_tokens_per_request: 1000,
                }
            }

            fn is_mock(&self) -> bool {
                false
            }
        }

        let directory = tempfile::tempdir().unwrap();
        let (store, _) = Store::initialize(directory.path().join("fail_dep.sqlite3")).unwrap();
        let orchestration = Orchestrator {
            store,
            provider: Arc::new(FailingDependencyProvider),
            python: get_python_exe().into(),
            worker_script: PathBuf::from("engine/app.py"),
            worker_timeout: Duration::from_secs(5),
            max_concurrency: 1, // Serial mode to test dependency failure handling in serial mode too
        };

        let record = orchestration
            .investigate("Verify failed dependency handling".into())
            .await
            .unwrap();

        assert_eq!(record.status, InvestigationStatus::Completed);
        assert_eq!(record.specialist_reports.len(), 2);

        let parent_report = record
            .specialist_reports
            .iter()
            .find(|r| r.specialist == Specialist::Physics)
            .unwrap();
        assert_eq!(parent_report.status, AgentExecutionStatus::Failed);

        let dep_report = record
            .specialist_reports
            .iter()
            .find(|r| r.specialist == Specialist::Mathematics)
            .unwrap();
        assert_eq!(dep_report.status, AgentExecutionStatus::Failed);
        assert!(
            dep_report
                .analysis
                .contains("Dependency 'step-fail' failed")
        );
    }

    #[test]
    fn test_every_terminal_stop_reason_maps_honestly() {
        use crate::cognition::StopReason;

        // 1. Genuine successful completions: all goals genuinely completed, zero cancelled, zero failed
        let (s, e) = map_stop_reason_to_investigation_outcome(
            Some(&StopReason::ObjectiveAchieved),
            true,
            false,
            false,
        );
        assert_eq!(s, InvestigationStatus::Completed);
        assert!(e.is_none());

        let (s, e) = map_stop_reason_to_investigation_outcome(
            Some(&StopReason::AllGoalsCompleted),
            true,
            false,
            false,
        );
        assert_eq!(s, InvestigationStatus::Completed);
        assert!(e.is_none());

        // 2. Cancelled-Goal Semantics: Cancelled goals must NEVER count as Completed!
        let (s, e) = map_stop_reason_to_investigation_outcome(
            Some(&StopReason::ObjectiveAchieved),
            false,
            true,
            false,
        );
        assert_eq!(s, InvestigationStatus::Interrupted);
        assert!(e.unwrap().contains("partially cancelled"));

        let (s, e) = map_stop_reason_to_investigation_outcome(
            Some(&StopReason::AllGoalsCompleted),
            false,
            true,
            false,
        );
        assert_eq!(s, InvestigationStatus::Interrupted);
        assert!(e.unwrap().contains("partially cancelled"));

        // 3. Failed-Goal Semantics: Failed goals must NEVER silently become Completed on AllGoalsCompleted!
        let (s, e) = map_stop_reason_to_investigation_outcome(
            Some(&StopReason::AllGoalsCompleted),
            false,
            false,
            true,
        );
        assert_eq!(s, InvestigationStatus::Failed);
        assert!(e.unwrap().contains("failed during investigation"));

        // 4. Budget exhaustion -> Interrupted (NOT Completed)
        let (s, e) = map_stop_reason_to_investigation_outcome(
            Some(&StopReason::BudgetExhausted),
            false,
            false,
            false,
        );
        assert_eq!(s, InvestigationStatus::Interrupted);
        assert!(e.unwrap().contains("budget exhausted"));

        // 5. Cycle limit -> Interrupted (NOT Completed)
        let (s, e) = map_stop_reason_to_investigation_outcome(
            Some(&StopReason::CycleLimitReached),
            false,
            false,
            false,
        );
        assert_eq!(s, InvestigationStatus::Interrupted);
        assert!(e.unwrap().contains("cycle limit reached"));

        // 6. Timeout -> Interrupted (NOT Completed)
        let (s, e) = map_stop_reason_to_investigation_outcome(
            Some(&StopReason::TimeoutExceeded),
            false,
            false,
            false,
        );
        assert_eq!(s, InvestigationStatus::Interrupted);
        assert!(e.unwrap().contains("timeout exceeded"));

        // 7. Deadlock -> Failed (NOT Completed)
        let (s, e) = map_stop_reason_to_investigation_outcome(
            Some(&StopReason::DeadlockNoExecutableActions),
            false,
            false,
            false,
        );
        assert_eq!(s, InvestigationStatus::Failed);
        assert!(e.unwrap().contains("Deadlock detected"));

        // 8. Metacognitive abort -> Failed (NOT Completed)
        let (s, e) = map_stop_reason_to_investigation_outcome(
            Some(&StopReason::MetacognitiveAbort {
                rationale: "Unresolvable contradiction threshold breached".into(),
            }),
            false,
            false,
            false,
        );
        assert_eq!(s, InvestigationStatus::Failed);
        assert!(e.unwrap().contains("contradiction"));

        // 9. User cancellation -> Interrupted (NOT Completed)
        let (s, e) = map_stop_reason_to_investigation_outcome(
            Some(&StopReason::UserCancelled),
            false,
            true,
            false,
        );
        assert_eq!(s, InvestigationStatus::Interrupted);
        assert!(e.unwrap().contains("cancelled"));

        // 10. None / unexpected stop with uncompleted goals -> Interrupted (NOT Completed)
        let (s, e) = map_stop_reason_to_investigation_outcome(None, false, false, false);
        assert_eq!(s, InvestigationStatus::Interrupted);
        assert!(e.unwrap().contains("unexpectedly"));

        // 11. None with all goals cancelled -> Interrupted
        let (s, e) = map_stop_reason_to_investigation_outcome(None, false, true, false);
        assert_eq!(s, InvestigationStatus::Interrupted);
        assert!(e.unwrap().contains("cancelled"));

        // 12. None with failed goals -> Failed
        let (s, e) = map_stop_reason_to_investigation_outcome(None, false, false, true);
        assert_eq!(s, InvestigationStatus::Failed);
        assert!(e.unwrap().contains("failed"));

        // 13. None with all goals genuinely completed -> Completed
        let (s, e) = map_stop_reason_to_investigation_outcome(None, true, false, false);
        assert_eq!(s, InvestigationStatus::Completed);
        assert!(e.is_none());
    }

    #[tokio::test]
    async fn test_runtime_error_propagation_prevents_completed_status() {
        let directory = tempfile::tempdir().unwrap();
        let db_path = directory.path().join("runtime_err.sqlite3");
        let (store, _) = Store::initialize(db_path.clone()).unwrap();

        // Install a controlled database trigger on cognitive_events:
        // Planning succeeds, but cognitive runtime persistence fails during runtime.run()
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TRIGGER abort_runtime_event
                 BEFORE INSERT ON cognitive_events
                 BEGIN
                     SELECT RAISE(ABORT, 'controlled persistence failure during runtime execution');
                 END;",
                [],
            )
            .unwrap();
        }

        let orchestration = Orchestrator {
            store: store.clone(),
            provider: Arc::new(MockProvider { max_tokens: 1000 }),
            python: get_python_exe().into(),
            worker_script: PathBuf::from("engine/app.py"),
            worker_timeout: Duration::from_secs(5),
            max_concurrency: 1,
        };

        // Planning succeeds with MockProvider, cognitive runtime starts, but runtime.run() fails mid-flight
        let result = orchestration
            .investigate("Investigate projectile trajectory".into())
            .await;
        assert!(
            result.is_err(),
            "Runtime failure during runtime.run() must propagate as Err"
        );
        let err_msg = result.unwrap_err();
        assert!(
            err_msg.contains("controlled persistence failure during runtime execution"),
            "Error must surface the actual runtime failure message: {err_msg}"
        );

        // Verify persisted record in SQLite is Failed, NOT Completed
        let history = store.list(None, 10).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, "failed");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let stored_error: Option<String> = conn
            .query_row(
                "SELECT error_json FROM investigations WHERE id=?1",
                [&history[0].id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            stored_error
                .as_ref()
                .unwrap()
                .contains("controlled persistence failure"),
            "Persisted error in SQLite must document the runtime failure: {:?}",
            stored_error
        );
    }

    #[tokio::test]
    async fn test_failure_persistence_surfaces_dual_errors() {
        struct FailingPlanProvider;
        #[async_trait::async_trait]
        impl ModelProvider for FailingPlanProvider {
            async fn complete(
                &self,
                _req: GenerationRequest,
                _budget: &RequestBudget,
            ) -> Result<provider::ModelCompletion, provider::ModelError> {
                Err(provider::ModelError::Http {
                    status: 500,
                    message: "Planning engine catastrophic failure".into(),
                })
            }
            fn settings(&self) -> ProviderSettings {
                ProviderSettings {
                    model_provider: "fail".into(),
                    model_name: Some("fail".into()),
                    memory_provider: "sqlite",
                    credentials_configured: true,
                    max_tokens_per_request: 1000,
                }
            }
            fn is_mock(&self) -> bool {
                false
            }
        }

        let directory = tempfile::tempdir().unwrap();
        let db_path = directory.path().join("dual_err.sqlite3");
        let (store, _) = Store::initialize(db_path.clone()).unwrap();

        // Install trigger that fails whenever updating investigations (e.g. when saving failure record)
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TRIGGER abort_investigations_update
                 BEFORE UPDATE ON investigations
                 BEGIN
                     SELECT RAISE(ABORT, 'controlled database update failure');
                 END;",
                [],
            )
            .unwrap();
        }

        let orchestration = Orchestrator {
            store: store.clone(),
            provider: Arc::new(FailingPlanProvider),
            python: get_python_exe().into(),
            worker_script: PathBuf::from("engine/app.py"),
            worker_timeout: Duration::from_secs(5),
            max_concurrency: 1,
        };

        let result = orchestration
            .investigate("Will this plan fail?".into())
            .await;
        assert!(result.is_err(), "Must return Err");
        let err_msg = result.unwrap_err();
        assert!(
            err_msg.contains("Planning engine catastrophic failure"),
            "Must surface primary error"
        );
        assert!(
            err_msg.contains("additionally failed to persist failure record"),
            "Must clearly surface persistence failure alongside primary error: {err_msg}"
        );
    }
}
