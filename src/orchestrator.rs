use std::{
    ffi::OsString,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    contracts::*,
    provider::{self, GenerationRequest, GenerationStage, ModelProvider, RequestBudget},
    store::Store,
    verification, worker,
};

pub struct Orchestrator {
    pub store: Store,
    pub provider: Arc<dyn ModelProvider>,
    pub python: OsString,
    pub worker_script: PathBuf,
    pub worker_timeout: Duration,
    pub max_concurrency: usize,
}

struct SpecialistTaskResult {
    subproblem_id: String,
    report: AgentReport,
    tools: Vec<ToolExecution>,
    verifications: Vec<VerificationResult>,
}

struct SpecialistContext {
    question: String,
    plan: InvestigationPlan,
    provider: Arc<dyn ModelProvider>,
    python: OsString,
    worker_script: PathBuf,
    worker_timeout: Duration,
    budget: Arc<RequestBudget>,
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

            if let Some(cs) = record.cognitive_state.as_mut() {
                cs.context.primary_objective = plan.objective.clone();
                cs.context.relevant_entities = plan.entities.clone();
                cs.context.known_variables = plan.known_variables.clone();
                cs.context.unknown_variables = plan.unknown_variables.clone();
                cs.context.constraints = plan.constraints.clone();
                cs.context.current_stage = "executing_subproblems".into();
                for sub in &plan.subproblems {
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
                        completion_criteria: vec![
                            crate::cognition::CompletionCriterion::SubGoalsCompleted,
                        ],
                        created_at: timestamp(),
                        completed_at: None,
                    };
                    let _ = cs.goals.add_goal(goal);
                }
            }

            self.store.save(&record).await.map_err(|e| e.to_string())?;

            // Task identity is defined by task IDs, not specialist names.
            let subproblems = plan.subproblems.clone();
            let total_count = subproblems.len();
            let effective_concurrency = self.max_concurrency.clamp(1, 5);

            let ctx = Arc::new(SpecialistContext {
                question: question.clone(),
                plan: plan.clone(),
                provider: self.provider.clone(),
                python: self.python.clone(),
                worker_script: self.worker_script.clone(),
                worker_timeout: self.worker_timeout,
                budget: budget.clone(),
            });

            // Bounded execution with dynamic dependency resolution, context propagation,
            // and failure isolation across both concurrent and serial modes.
            let mut pending = subproblems;
            let mut completed_ids = std::collections::HashSet::<String>::new();
            let mut failed_ids = std::collections::HashSet::<String>::new();
            let mut task_tools = std::collections::HashMap::<String, Vec<ToolExecution>>::new();
            let mut task_reports = std::collections::HashMap::<String, AgentReport>::new();
            let mut join_set: tokio::task::JoinSet<SpecialistTaskResult> =
                tokio::task::JoinSet::new();

            while completed_ids.len() < total_count {
                // 1. Surface failed dependencies: any pending task depending on a failed task fails immediately
                let mut newly_failed = Vec::new();
                let mut i = 0;
                while i < pending.len() {
                    let failed_dep = pending[i]
                        .depends_on
                        .iter()
                        .find(|dep| failed_ids.contains(*dep))
                        .cloned();
                    if let Some(dep_name) = failed_dep {
                        let failed_sub = pending.remove(i);
                        newly_failed.push((failed_sub, dep_name));
                    } else {
                        i += 1;
                    }
                }

                for (failed_sub, dep_name) in newly_failed {
                    failed_ids.insert(failed_sub.id.clone());
                    completed_ids.insert(failed_sub.id.clone());
                    record.specialist_reports.push(AgentReport {
                        specialist: failed_sub.specialist,
                        status: AgentExecutionStatus::Failed,
                        analysis: format!(
                            "Dependency '{}' failed, preventing execution.",
                            dep_name
                        ),
                        claims: vec![],
                        tool_calls: vec![],
                        assumptions: vec![],
                        uncertainties: vec![format!("Dependency failure: {dep_name}")],
                        evidence_requirements: vec![],
                        duration_ms: 0,
                    });
                }

                if completed_ids.len() >= total_count {
                    break;
                }

                // 2. Spawn ready subproblems up to effective_concurrency
                let mut i = 0;
                while i < pending.len() && join_set.len() < effective_concurrency {
                    let is_ready = pending[i]
                        .depends_on
                        .iter()
                        .all(|dep| completed_ids.contains(dep));

                    if is_ready {
                        let sub = pending.remove(i);
                        let sub_ctx = ctx.clone();

                        // Collect dependency tools (direct and transitive) and dependency reports
                        let mut dep_tools = Vec::new();
                        let mut dep_reports = Vec::new();
                        let mut dep_set = std::collections::HashSet::new();
                        let mut queue = sub.depends_on.clone();
                        while let Some(dep_id) = queue.pop() {
                            if dep_set.insert(dep_id.clone())
                                && let Some(parent_sub) =
                                    plan.subproblems.iter().find(|s| s.id == dep_id)
                            {
                                queue.extend(parent_sub.depends_on.clone());
                            }
                        }
                        for p_sub in &plan.subproblems {
                            if dep_set.contains(&p_sub.id) {
                                if let Some(tools) = task_tools.get(&p_sub.id) {
                                    dep_tools.extend(tools.clone());
                                }
                                if let Some(rep) = task_reports.get(&p_sub.id) {
                                    dep_reports.push(rep.clone());
                                }
                            }
                        }

                        join_set.spawn(async move {
                            run_specialist_subproblem(sub, sub_ctx, dep_tools, dep_reports).await
                        });
                    } else {
                        i += 1;
                    }
                }

                if join_set.is_empty() {
                    // If no tasks are currently executing and nothing in pending is ready,
                    // handle unsatisfied dependencies with failure isolation
                    for rem in pending.drain(..) {
                        completed_ids.insert(rem.id.clone());
                        failed_ids.insert(rem.id.clone());
                        record.specialist_reports.push(AgentReport {
                            specialist: rem.specialist,
                            status: AgentExecutionStatus::Failed,
                            analysis: "Unsatisfied subproblem dependency prevented execution."
                                .into(),
                            claims: vec![],
                            tool_calls: vec![],
                            assumptions: vec![],
                            uncertainties: vec!["Dependency resolution failure".into()],
                            evidence_requirements: vec![],
                            duration_ms: 0,
                        });
                    }
                    break;
                }

                // 3. Await the next completing specialist task
                if let Some(join_res) = join_set.join_next().await {
                    match join_res {
                        Ok(task_res) => {
                            if task_res.report.status == AgentExecutionStatus::Failed {
                                failed_ids.insert(task_res.subproblem_id.clone());
                            }
                            completed_ids.insert(task_res.subproblem_id.clone());
                            task_tools
                                .insert(task_res.subproblem_id.clone(), task_res.tools.clone());
                            task_reports
                                .insert(task_res.subproblem_id.clone(), task_res.report.clone());
                            if let Some(cs) = record.cognitive_state.as_mut() {
                                if let Some(goal) = cs.goals.get_goal_mut(&task_res.subproblem_id)
                                {
                                    if task_res.report.status == AgentExecutionStatus::Failed {
                                        goal.status = crate::cognition::GoalStatus::Failed;
                                    } else {
                                        goal.status = crate::cognition::GoalStatus::Completed;
                                        goal.completed_at = Some(timestamp());
                                    }
                                }
                                for tool in &task_res.tools {
                                    let k_item = crate::cognition::KnowledgeItem {
                                        id: tool.id.clone(),
                                        item_type: crate::cognition::KnowledgeItemType::Calculation,
                                        content: serde_json::to_value(tool).unwrap_or_default(),
                                        provenance: crate::cognition::Provenance::new(
                                            "specialist_tool",
                                        )
                                        .with_source_id(tool.specialist.as_str()),
                                        epistemic_status: tool.epistemic_status.clone(),
                                        related_goals: vec![task_res.subproblem_id.clone()],
                                        related_entities: vec![],
                                        timestamp: timestamp(),
                                    };
                                    cs.add_knowledge_item(k_item);
                                }
                            }
                            record.specialist_reports.push(task_res.report);
                            record.tools.extend(task_res.tools);
                            record.verification.extend(task_res.verifications);
                            record.updated_at = timestamp();
                            let _ = self.store.save(&record).await;
                        }
                        Err(join_err) => {
                            tracing::error!(%join_err, "Specialist task panicked or was cancelled");
                        }
                    }
                }
            }

            // Synthesis phase
            record.status = InvestigationStatus::Synthesizing;
            record.updated_at = timestamp();
            self.store.save(&record).await.map_err(|e| e.to_string())?;

            let mut synthesis: Synthesis = provider::generate_structured(
                self.provider.as_ref(),
                GenerationRequest {
                    stage: GenerationStage::Synthesis,
                    specialist: None,
                    system_instruction: "Synthesize specialist reports and computed tool outputs. Clearly separate mock output, hypotheses, calculations, evidence, assumptions, disagreements, uncertainty, and next steps. Do not claim evidence from unavailable sources. Return fields executive_summary,key_findings,disagreements,assumptions,uncertainties,next_steps.".into(),
                    input: json!({
                        "question": question,
                        "plan": plan,
                        "specialist_reports": record.specialist_reports,
                        "tools": record.tools,
                        "sources": record.sources,
                    }),
                },
                budget.as_ref(),
            )
            .await
            .map_err(|e| e.to_string())?;

            // Strict epistemic rules for synthesis findings:
            // A model can never self-assign 'verified' status.
            for claim in &mut synthesis.key_findings {
                let backed = claim.source_ids.iter().any(|id| {
                    record
                        .sources
                        .iter()
                        .any(|source| source.get("id").and_then(Value::as_str) == Some(id))
                });
                let verified = claim.tool_ids.iter().any(|id| {
                    record.tools.iter().any(|tool| {
                        &tool.id == id && tool.epistemic_status == EpistemicStatus::Verified
                    })
                });
                let calculated = claim.tool_ids.iter().any(|id| {
                    record.tools.iter().any(|tool| {
                        &tool.id == id && tool.epistemic_status == EpistemicStatus::Calculated
                    })
                });

                if !backed {
                    claim.epistemic_status = if verified {
                        EpistemicStatus::Verified
                    } else if calculated {
                        EpistemicStatus::Calculated
                    } else {
                        EpistemicStatus::Hypothesis
                    };
                }
            }

            record.synthesis = Some(synthesis);
            record.status = InvestigationStatus::Completed;
            record.updated_at = timestamp();
            record.model.request_count = budget.model_call_count();
            record.model.http_attempt_count = budget.http_attempt_count();
            record.model.usage = budget.usage();

            if let Some(cs) = record.cognitive_state.as_mut() {
                cs.context.current_stage = "completed".into();
                cs.executive.current_phase = crate::cognition::ExecutivePhase::Stopped;
                cs.executive.stop_reason = Some(crate::cognition::StopReason::ObjectiveAchieved);
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
                let _ = self.store.save(&record).await;
                Err(error)
            }
        }
    }
}

async fn run_specialist_subproblem(
    sub: ResearchSubproblem,
    ctx: Arc<SpecialistContext>,
    prior_tools: Vec<ToolExecution>,
    prior_reports: Vec<AgentReport>,
) -> SpecialistTaskResult {
    let start = Instant::now();

    let out_res: Result<SpecialistOutput, _> = provider::generate_structured(
        ctx.provider.as_ref(),
        GenerationRequest {
            stage: GenerationStage::Specialist,
            specialist: Some(sub.specialist.clone()),
            system_instruction: format!(
                "{} Return JSON matching {{analysis,claims,tool_calls,assumptions,uncertainties,evidence_requirements}}. Claims must not be labelled verified or evidence_supported without independent proof or actual sources. Allowed tools: {:?}. Never issue code or shell commands.",
                sub.specialist.instructions(),
                sub.specialist.allowed_tools()
            ),
            input: json!({
                "question": ctx.question,
                "task": sub.task,
                "plan": ctx.plan,
                "tool_results": prior_tools,
                "dependency_reports": prior_reports,
            }),
        },
        ctx.budget.as_ref(),
    )
    .await;

    let out = match out_res {
        Ok(o) => o,
        Err(err) => {
            // Failure isolation: record failure without terminating other tasks
            let report = AgentReport {
                specialist: sub.specialist.clone(),
                status: AgentExecutionStatus::Failed,
                analysis: format!("Specialist model execution failed: {err}"),
                claims: vec![],
                tool_calls: vec![],
                assumptions: vec![],
                uncertainties: vec![format!("Model failure: {err}")],
                evidence_requirements: vec![],
                duration_ms: start.elapsed().as_millis() as u64,
            };
            return SpecialistTaskResult {
                subproblem_id: sub.id,
                report,
                tools: vec![],
                verifications: vec![],
            };
        }
    };

    let mut report = AgentReport {
        specialist: sub.specialist.clone(),
        status: if ctx.provider.is_mock() {
            AgentExecutionStatus::Mock
        } else {
            AgentExecutionStatus::Completed
        },
        analysis: out.analysis,
        claims: out.claims,
        tool_calls: out.tool_calls.clone(),
        assumptions: out.assumptions,
        uncertainties: out.uncertainties,
        evidence_requirements: out.evidence_requirements,
        duration_ms: start.elapsed().as_millis() as u64,
    };

    let mut tools = Vec::new();
    let mut verifications = Vec::new();

    for (call_index, call) in out.tool_calls.into_iter().enumerate() {
        if call_index >= 3 {
            tools.push(ToolExecution {
                id: Uuid::new_v4().to_string(),
                specialist: sub.specialist.clone(),
                tool: call.tool,
                arguments: call.arguments,
                output: None,
                method: None,
                epistemic_status: EpistemicStatus::Uncertain,
                verification_id: None,
                error: Some("Specialist tool-call limit reached (3 per role)".into()),
                duration_ms: 0,
            });
            continue;
        }

        if !sub.specialist.allowed_tools().contains(&call.tool.as_str()) {
            tools.push(ToolExecution {
                id: Uuid::new_v4().to_string(),
                specialist: sub.specialist.clone(),
                tool: call.tool,
                arguments: call.arguments,
                output: None,
                method: None,
                epistemic_status: EpistemicStatus::Uncertain,
                verification_id: None,
                error: Some("Tool rejected by the specialist allowlist".into()),
                duration_ms: 0,
            });
            continue;
        }

        let started = Instant::now();
        let invoke_res = worker::invoke(
            ctx.python.clone(),
            ctx.worker_script.clone(),
            "/api/tool",
            json!({"name": call.tool, "args": call.arguments}),
            ctx.worker_timeout,
        )
        .await;

        let (worker_val, err_msg) = match invoke_res {
            Ok(val) => {
                let output = val.get("result").cloned().unwrap_or(val);
                (Ok(output), None)
            }
            Err(e) => (Err(format!("{e:?}")), Some(format!("{e:?}"))),
        };

        // Independent deterministic verification in Rust
        let (v_res, epistemic_status) =
            verification::verify_tool(&call.tool, &call.arguments, &worker_val);
        let v_id = v_res.id.clone();
        verifications.push(v_res);

        let output_val = worker_val.ok();
        let method = output_val
            .as_ref()
            .and_then(|v| v.get("method").and_then(Value::as_str))
            .map(str::to_owned);

        tools.push(ToolExecution {
            id: Uuid::new_v4().to_string(),
            specialist: sub.specialist.clone(),
            tool: call.tool,
            arguments: call.arguments,
            output: output_val,
            method,
            epistemic_status,
            verification_id: Some(v_id),
            error: err_msg,
            duration_ms: started.elapsed().as_millis() as u64,
        });
    }

    // Epistemic status enforcement for specialist claims
    // A model cannot claim 'verified' without an independently verified tool
    for claim in &mut report.claims {
        let backed = false; // Scholarly retrieval is configured in later milestones
        let verified = claim.tool_ids.iter().any(|id| {
            tools
                .iter()
                .any(|t| &t.id == id && t.epistemic_status == EpistemicStatus::Verified)
        });
        let calculated = claim.tool_ids.iter().any(|id| {
            tools
                .iter()
                .any(|t| &t.id == id && t.epistemic_status == EpistemicStatus::Calculated)
        });

        if !backed {
            claim.epistemic_status = if verified {
                EpistemicStatus::Verified
            } else if calculated {
                EpistemicStatus::Calculated
            } else {
                EpistemicStatus::Hypothesis
            };
        }
    }

    SpecialistTaskResult {
        subproblem_id: sub.id,
        report,
        tools,
        verifications,
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
}
