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
    prior_tools: Vec<ToolExecution>,
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
        };
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
            self.store.save(&record).await.map_err(|e| e.to_string())?;

            // Deduplicate subproblems by specialist role to preserve bounded execution
            let mut seen = std::collections::HashSet::new();
            let mut unique_subproblems = Vec::new();
            for sub in &plan.subproblems {
                if seen.insert(sub.specialist.as_str()) {
                    unique_subproblems.push(sub.clone());
                }
            }

            let total_count = unique_subproblems.len();
            let effective_concurrency = self.max_concurrency.clamp(1, 5);

            let ctx = Arc::new(SpecialistContext {
                question: question.clone(),
                plan: plan.clone(),
                prior_tools: record.tools.clone(),
                provider: self.provider.clone(),
                python: self.python.clone(),
                worker_script: self.worker_script.clone(),
                worker_timeout: self.worker_timeout,
                budget: budget.clone(),
            });

            if effective_concurrency <= 1 {
                // Serial execution mode
                for sub in unique_subproblems {
                    let task_res = run_specialist_subproblem(sub, ctx.clone()).await;

                    record.specialist_reports.push(task_res.report);
                    record.tools.extend(task_res.tools);
                    record.verification.extend(task_res.verifications);
                    record.updated_at = timestamp();
                    let _ = self.store.save(&record).await;
                }
            } else {
                // Bounded asynchronous execution with dynamic dependency resolution
                let mut pending = unique_subproblems;
                let mut completed_ids = std::collections::HashSet::new();
                let mut join_set: tokio::task::JoinSet<SpecialistTaskResult> =
                    tokio::task::JoinSet::new();

                while completed_ids.len() < total_count {
                    // Spawn all ready subproblems up to max_concurrency
                    let mut i = 0;
                    while i < pending.len() && join_set.len() < effective_concurrency {
                        let is_ready = pending[i]
                            .depends_on
                            .iter()
                            .all(|dep| completed_ids.contains(dep));

                        if is_ready {
                            let sub = pending.remove(i);
                            let sub_ctx = ctx.clone();

                            join_set.spawn(async move {
                                run_specialist_subproblem(sub, sub_ctx).await
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

                    // Await the next completing specialist task
                    if let Some(join_res) = join_set.join_next().await {
                        match join_res {
                            Ok(task_res) => {
                                completed_ids.insert(task_res.subproblem_id);
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
                let _ = self.store.save(&record).await;
                Err(error)
            }
        }
    }
}

async fn run_specialist_subproblem(
    sub: ResearchSubproblem,
    ctx: Arc<SpecialistContext>,
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
                "tool_results": ctx.prior_tools
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
}
