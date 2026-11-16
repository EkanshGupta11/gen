use std::{
    ffi::OsString,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    cognition::{
        actions::{ActionExecutionStatus, ActionResult, ExecutiveAction},
        goals::{CompletionCriterion, Goal, GoalStatus},
        policy::ALLOWED_SCIENTIFIC_TOOLS,
        state::{
            CognitiveState, ContradictionResolution, KnowledgeItem, Provenance, ResolutionOutcome,
        },
        working_memory::KnowledgeItemType,
    },
    contracts::{AgentExecutionStatus, AgentReport, EpistemicStatus, ToolExecution},
    provider::{self, GenerationRequest, GenerationStage, ModelProvider, RequestBudget},
    verification, worker,
};

#[derive(Clone)]
pub struct AsyncActionExecutor {
    python: OsString,
    worker_script: PathBuf,
    worker_timeout: Duration,
    provider: Arc<dyn ModelProvider>,
    budget: Arc<RequestBudget>,
}

impl AsyncActionExecutor {
    pub fn new(
        python: OsString,
        worker_script: PathBuf,
        worker_timeout: Duration,
        provider: Arc<dyn ModelProvider>,
        budget: Arc<RequestBudget>,
    ) -> Self {
        Self {
            python,
            worker_script,
            worker_timeout,
            provider,
            budget,
        }
    }

    pub fn for_testing() -> Self {
        Self {
            python: "python".into(),
            worker_script: "engine/app.py".into(),
            worker_timeout: Duration::from_secs(5),
            provider: Arc::new(provider::MockProvider {
                max_tokens: 100_000,
            }),
            budget: Arc::new(RequestBudget::new(100_000)),
        }
    }

    /// Execute an executive action asynchronously against real tools, checkers, or providers.
    pub async fn execute(
        &self,
        action: &ExecutiveAction,
        state: &mut CognitiveState,
    ) -> ActionResult {
        let start = Instant::now();
        let action_id = Uuid::new_v4().to_string();

        match action {
            ExecutiveAction::ExecuteScientificTool {
                tool,
                arguments,
                goal_id,
            } => {
                // 1. Allowlist validation
                if !ALLOWED_SCIENTIFIC_TOOLS.contains(&tool.as_str()) {
                    return ActionResult::unsupported(
                        action_id,
                        action.name(),
                        format!(
                            "Tool '{tool}' is not in the allowed scientific tools allowlist: {ALLOWED_SCIENTIFIC_TOOLS:?}"
                        ),
                        start.elapsed().as_millis() as u64,
                    );
                }

                // 2. Argument validation
                if !arguments.is_object() {
                    return ActionResult::failed(
                        action_id,
                        action.name(),
                        format!("Arguments for tool '{tool}' must be a JSON object"),
                        start.elapsed().as_millis() as u64,
                    );
                }

                // 3. Real Python worker invocation
                let invoke_res = worker::invoke(
                    self.python.clone(),
                    self.worker_script.clone(),
                    "/api/tool",
                    json!({"name": tool, "args": arguments}),
                    self.worker_timeout,
                )
                .await;

                let (worker_val, err_msg) = match invoke_res {
                    Ok(val) => {
                        let output = val.get("result").cloned().unwrap_or(val);
                        (Ok(output), None)
                    }
                    Err(e) => (Err(format!("{e:?}")), Some(format!("{e:?}"))),
                };

                // 4. Independent deterministic Rust verification
                let (v_res, epistemic_status) =
                    verification::verify_tool(tool, arguments, &worker_val);

                let duration_ms = start.elapsed().as_millis() as u64;

                let _output_val = match &worker_val {
                    Ok(val) => val.clone(),
                    Err(e) => json!({"error": e}),
                };

                let k_item_id = format!("calc-{}", Uuid::new_v4());
                let t_exec = crate::contracts::ToolExecution {
                    id: k_item_id.clone(),
                    specialist: crate::contracts::Specialist::Mathematics,
                    tool: tool.clone(),
                    arguments: arguments.clone(),
                    output: worker_val.ok(),
                    method: None,
                    epistemic_status: epistemic_status.clone(),
                    verification_id: Some(v_res.id.clone()),
                    error: err_msg.clone(),
                    duration_ms,
                };

                let mut tool_json = serde_json::to_value(&t_exec).unwrap_or_default();
                if let Some(obj) = tool_json.as_object_mut() {
                    obj.insert(
                        "verification".into(),
                        serde_json::to_value(&v_res).unwrap_or_default(),
                    );
                }

                // 5. Epistemic Classification and Knowledge Storage
                let k_item = KnowledgeItem {
                    id: k_item_id.clone(),
                    item_type: KnowledgeItemType::Calculation,
                    content: tool_json.clone(),
                    provenance: Provenance::new("scientific_tool").with_source_id(tool),
                    epistemic_status: epistemic_status.clone(),
                    related_goals: vec![goal_id.clone()],
                    related_entities: vec![],
                    timestamp: time::OffsetDateTime::now_utc()
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                };
                state.add_knowledge_item(k_item);

                // 6. If contradicted, record contradiction
                if epistemic_status == EpistemicStatus::Contradicted {
                    state.record_contradiction(
                        format!("contra-{}", Uuid::new_v4()),
                        format!(
                            "Tool '{tool}' calculation contradicted independent verification: {}",
                            v_res.details
                        ),
                        vec![k_item_id],
                    );
                    return ActionResult::contradicted(
                        action_id,
                        action.name(),
                        tool_json,
                        format!(
                            "Calculation contradicted independent verification: {}",
                            v_res.details
                        ),
                        duration_ms,
                    );
                }

                if let Some(err) = err_msg {
                    return ActionResult::failed(action_id, action.name(), err, duration_ms);
                }

                ActionResult::succeeded(action_id, action.name(), tool_json, duration_ms)
            }

            ExecutiveAction::VerifyCalculation { tool_id, goal_id } => {
                let duration_ms = start.elapsed().as_millis() as u64;

                // 1. Recover original calculation item by ID or by tool name / source_id
                let item_opt = state
                    .knowledge_items
                    .get(tool_id)
                    .or_else(|| {
                        state.knowledge_items.values().find(|k| {
                            k.content.get("tool").and_then(Value::as_str) == Some(tool_id)
                                || k.provenance.source_id.as_deref() == Some(tool_id)
                        })
                    })
                    .cloned();

                let item = match item_opt {
                    Some(it) => it,
                    None => {
                        return ActionResult::failed(
                            action_id,
                            action.name(),
                            format!("Calculation knowledge item '{tool_id}' not found"),
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
                        .unwrap_or("Worker output contained error")
                        .to_string()),
                    None => Err("No output recorded in calculation item".to_string()),
                };

                // 2. Invoke independent Rust checker
                let (v_res, epistemic_status) =
                    verification::verify_tool(&tool_name, &arguments, &worker_res);

                // 3. Update knowledge item and working memory item
                if let Some(k_item) = state.knowledge_items.get_mut(&item.id) {
                    k_item.epistemic_status = epistemic_status.clone();
                    if let Some(obj) = k_item.content.as_object_mut() {
                        obj.insert("epistemic_status".into(), json!(epistemic_status.as_str()));
                        obj.insert(
                            "verification".into(),
                            serde_json::to_value(&v_res).unwrap_or_default(),
                        );
                        obj.insert("verification_id".into(), json!(v_res.id));
                    }
                }
                if let Some(wm_item) = state.working_memory.get_mut(&item.id) {
                    wm_item.epistemic_status = epistemic_status.clone();
                }

                let output = json!({
                    "verified_tool_id": item.id,
                    "goal_id": goal_id,
                    "verification": v_res,
                    "epistemic_status": epistemic_status.as_str()
                });

                if epistemic_status == EpistemicStatus::Contradicted {
                    state.record_contradiction(
                        format!("contra-{}", Uuid::new_v4()),
                        format!(
                            "Tool '{}' contradicted independent verification: {}",
                            item.id, v_res.details
                        ),
                        vec![item.id.clone()],
                    );
                    return ActionResult::contradicted(
                        action_id,
                        action.name(),
                        output,
                        format!(
                            "Calculation contradicted independent verification: {}",
                            v_res.details
                        ),
                        duration_ms,
                    );
                }

                if epistemic_status == EpistemicStatus::Verified {
                    ActionResult::succeeded(action_id, action.name(), output, duration_ms)
                } else {
                    ActionResult::new(
                        action_id,
                        action.name(),
                        ActionExecutionStatus::Uncertain,
                        output,
                        Some(
                            "Independent checker could not verify output or checker unavailable"
                                .into(),
                        ),
                        duration_ms,
                    )
                }
            }

            ExecutiveAction::ResolveContradiction {
                contradiction_id,
                resolution_strategy,
            } => {
                let duration_ms = start.elapsed().as_millis() as u64;

                let contradiction_opt = state
                    .contradictions
                    .iter()
                    .position(|c| &c.id == contradiction_id);

                let idx = match contradiction_opt {
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

                let conflicting_ids = state.contradictions[idx].conflicting_items.clone();
                let mut verified_evidence = Vec::new();
                let mut contradicted_items = Vec::new();

                for item_id in &conflicting_ids {
                    if let Some(item) = state.knowledge_items.get(item_id) {
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

                if !verified_evidence.is_empty() {
                    let resolution = ContradictionResolution {
                        contradiction_id: contradiction_id.clone(),
                        conflicting_items: conflicting_ids.clone(),
                        proposed_resolution: format!(
                            "Arbitrated via {resolution_strategy}: retained verified evidence {:?}, rejected contradicted items {:?}",
                            verified_evidence, contradicted_items
                        ),
                        supporting_evidence_refs: verified_evidence,
                        verification_method: "rust_independent_verification".into(),
                        outcome: ResolutionOutcome::Resolved,
                        timestamp: now,
                    };
                    state.contradictions[idx].resolved = true;
                    state.contradictions[idx].resolution = Some(resolution.clone());

                    ActionResult::succeeded(
                        action_id,
                        action.name(),
                        serde_json::to_value(&resolution).unwrap_or_default(),
                        duration_ms,
                    )
                } else {
                    let resolution = ContradictionResolution {
                        contradiction_id: contradiction_id.clone(),
                        conflicting_items: conflicting_ids.clone(),
                        proposed_resolution: format!(
                            "Insufficient verified evidence to resolve via {resolution_strategy}. Requires human review."
                        ),
                        supporting_evidence_refs: vec![],
                        verification_method: "none".into(),
                        outcome: ResolutionOutcome::RequiresHumanReview,
                        timestamp: now,
                    };
                    state.contradictions[idx].resolved = false;
                    state.contradictions[idx].resolution = Some(resolution.clone());

                    ActionResult::new(
                        action_id,
                        action.name(),
                        ActionExecutionStatus::Uncertain,
                        serde_json::to_value(&resolution).unwrap_or_default(),
                        Some("Cannot resolve contradiction without verified evidence".into()),
                        duration_ms,
                    )
                }
            }

            ExecutiveAction::RequestSpecialistAnalysis {
                specialist,
                task,
                goal_id,
            } => {
                let duration_ms = start.elapsed().as_millis() as u64;

                // Collect dependencies of this goal (direct and transitive)
                let mut dep_set = std::collections::HashSet::new();
                if let Some(goal) = state.goals.get_goal(goal_id) {
                    let mut queue = goal.dependencies.clone();
                    while let Some(dep_id) = queue.pop() {
                        if dep_set.insert(dep_id.clone())
                            && let Some(dep_goal) = state.goals.get_goal(&dep_id)
                        {
                            queue.extend(dep_goal.dependencies.clone());
                        }
                    }
                }

                // Gather prior tools and reports from dependencies
                let mut dep_tools = Vec::new();
                let mut dep_reports = Vec::new();

                for k in state.knowledge_items.values() {
                    let matches_dep = if dep_set.is_empty() {
                        false
                    } else {
                        k.related_goals.iter().any(|rg| dep_set.contains(rg))
                    };

                    if matches_dep {
                        if k.item_type == KnowledgeItemType::Calculation {
                            dep_tools.push(k.content.clone());
                        } else if k.provenance.source_type == "specialist_report" {
                            dep_reports.push(k.content.clone());
                        }
                    }
                }

                let out_res: Result<crate::contracts::SpecialistOutput, _> =
                    provider::generate_structured(
                        self.provider.as_ref(),
                        GenerationRequest {
                            stage: GenerationStage::Specialist,
                            specialist: Some(specialist.clone()),
                            system_instruction: format!(
                                "{} Return JSON matching {{analysis,claims,tool_calls,assumptions,uncertainties,evidence_requirements}}. Claims must not be labelled verified or evidence_supported without independent proof or actual sources. Allowed tools: {:?}. Never issue code or shell commands.",
                                specialist.instructions(),
                                specialist.allowed_tools()
                            ),
                            input: json!({
                                "question": state.context.primary_objective,
                                "task": task,
                                "tool_results": dep_tools,
                                "dependency_reports": dep_reports,
                            }),
                        },
                        self.budget.as_ref(),
                    )
                    .await;

                match out_res {
                    Ok(mut output) => {
                        let mut tool_executions = Vec::new();

                        for (call_index, call) in output.tool_calls.iter().enumerate() {
                            if call_index >= 3 {
                                let t_exec = ToolExecution {
                                    id: Uuid::new_v4().to_string(),
                                    specialist: specialist.clone(),
                                    tool: call.tool.clone(),
                                    arguments: call.arguments.clone(),
                                    output: None,
                                    method: None,
                                    epistemic_status: EpistemicStatus::Uncertain,
                                    verification_id: None,
                                    error: Some(
                                        "Specialist tool-call limit reached (3 per role)".into(),
                                    ),
                                    duration_ms: 0,
                                };
                                tool_executions.push(t_exec);
                                continue;
                            }

                            if !specialist.allowed_tools().contains(&call.tool.as_str()) {
                                let t_exec = ToolExecution {
                                    id: Uuid::new_v4().to_string(),
                                    specialist: specialist.clone(),
                                    tool: call.tool.clone(),
                                    arguments: call.arguments.clone(),
                                    output: None,
                                    method: None,
                                    epistemic_status: EpistemicStatus::Uncertain,
                                    verification_id: None,
                                    error: Some("Tool rejected by the specialist allowlist".into()),
                                    duration_ms: 0,
                                };
                                tool_executions.push(t_exec);
                                continue;
                            }

                            let started = Instant::now();
                            let invoke_res = worker::invoke(
                                self.python.clone(),
                                self.worker_script.clone(),
                                "/api/tool",
                                json!({"name": call.tool, "args": call.arguments}),
                                self.worker_timeout,
                            )
                            .await;

                            let (worker_val, err_msg) = match invoke_res {
                                Ok(val) => {
                                    let out = val.get("result").cloned().unwrap_or(val);
                                    (Ok(out), None)
                                }
                                Err(e) => (Err(format!("{e:?}")), Some(format!("{e:?}"))),
                            };

                            let (v_res, epistemic_status) =
                                verification::verify_tool(&call.tool, &call.arguments, &worker_val);
                            let v_id = v_res.id.clone();

                            let output_val = worker_val.ok();
                            let method = output_val
                                .as_ref()
                                .and_then(|v| v.get("method").and_then(Value::as_str))
                                .map(str::to_owned);

                            let t_id = Uuid::new_v4().to_string();
                            let t_exec = ToolExecution {
                                id: t_id.clone(),
                                specialist: specialist.clone(),
                                tool: call.tool.clone(),
                                arguments: call.arguments.clone(),
                                output: output_val.clone(),
                                method,
                                epistemic_status: epistemic_status.clone(),
                                verification_id: Some(v_id),
                                error: err_msg,
                                duration_ms: started.elapsed().as_millis() as u64,
                            };
                            tool_executions.push(t_exec.clone());

                            // Store calculation in knowledge items
                            let mut tool_json = serde_json::to_value(&t_exec).unwrap_or_default();
                            if let Some(obj) = tool_json.as_object_mut() {
                                obj.insert(
                                    "verification".into(),
                                    serde_json::to_value(&v_res).unwrap_or_default(),
                                );
                            }
                            let k_item = KnowledgeItem {
                                id: t_id.clone(),
                                item_type: KnowledgeItemType::Calculation,
                                content: tool_json,
                                provenance: Provenance::new("specialist_tool")
                                    .with_source_id(specialist.as_str()),
                                epistemic_status: epistemic_status.clone(),
                                related_goals: vec![goal_id.clone()],
                                related_entities: vec![],
                                timestamp: time::OffsetDateTime::now_utc()
                                    .format(&time::format_description::well_known::Rfc3339)
                                    .unwrap_or_default(),
                            };
                            state.add_knowledge_item(k_item);

                            if epistemic_status == EpistemicStatus::Contradicted {
                                state.record_contradiction(
                                    format!("contra-{}", Uuid::new_v4()),
                                    format!("Tool '{}' contradicted verification", call.tool),
                                    vec![t_id],
                                );
                            }
                        }

                        // Specialist claim epistemic enforcement
                        for claim in &mut output.claims {
                            let verified = claim.tool_ids.iter().any(|id| {
                                tool_executions.iter().any(|t| {
                                    &t.id == id && t.epistemic_status == EpistemicStatus::Verified
                                })
                            });
                            let calculated = claim.tool_ids.iter().any(|id| {
                                tool_executions.iter().any(|t| {
                                    &t.id == id && t.epistemic_status == EpistemicStatus::Calculated
                                })
                            });
                            if !claim.tool_ids.is_empty() {
                                claim.epistemic_status = if verified {
                                    EpistemicStatus::Verified
                                } else if calculated {
                                    EpistemicStatus::Calculated
                                } else {
                                    EpistemicStatus::Hypothesis
                                };
                            }
                        }

                        let report = AgentReport {
                            specialist: specialist.clone(),
                            status: if self.provider.is_mock() {
                                AgentExecutionStatus::Mock
                            } else {
                                AgentExecutionStatus::Completed
                            },
                            analysis: output.analysis,
                            claims: output.claims,
                            tool_calls: output.tool_calls,
                            assumptions: output.assumptions,
                            uncertainties: output.uncertainties,
                            evidence_requirements: output.evidence_requirements,
                            duration_ms,
                        };

                        let report_val = serde_json::to_value(&report).unwrap_or_default();
                        let k_item = KnowledgeItem {
                            id: format!("report-{}", Uuid::new_v4()),
                            item_type: KnowledgeItemType::Fact,
                            content: report_val.clone(),
                            provenance: Provenance::new("specialist_report")
                                .with_source_id(specialist.as_str()),
                            epistemic_status: if self.provider.is_mock() {
                                EpistemicStatus::Hypothesis
                            } else {
                                EpistemicStatus::Uncertain
                            },
                            related_goals: vec![goal_id.clone()],
                            related_entities: vec![],
                            timestamp: time::OffsetDateTime::now_utc()
                                .format(&time::format_description::well_known::Rfc3339)
                                .unwrap_or_default(),
                        };
                        state.add_knowledge_item(k_item);

                        ActionResult::succeeded(action_id, action.name(), report_val, duration_ms)
                    }
                    Err(e) => {
                        let failed_report = AgentReport {
                            specialist: specialist.clone(),
                            status: AgentExecutionStatus::Failed,
                            analysis: format!("Specialist analysis failed: {e}"),
                            claims: vec![],
                            tool_calls: vec![],
                            assumptions: vec![],
                            uncertainties: vec![format!("Execution failure: {e}")],
                            evidence_requirements: vec![],
                            duration_ms,
                        };
                        let report_val = serde_json::to_value(&failed_report).unwrap_or_default();
                        let k_item = KnowledgeItem {
                            id: format!("report-{}", Uuid::new_v4()),
                            item_type: KnowledgeItemType::Fact,
                            content: report_val.clone(),
                            provenance: Provenance::new("specialist_report")
                                .with_source_id(specialist.as_str()),
                            epistemic_status: EpistemicStatus::Uncertain,
                            related_goals: vec![goal_id.clone()],
                            related_entities: vec![],
                            timestamp: time::OffsetDateTime::now_utc()
                                .format(&time::format_description::well_known::Rfc3339)
                                .unwrap_or_default(),
                        };
                        state.add_knowledge_item(k_item);

                        if let Some(goal) = state.goals.get_goal_mut(goal_id) {
                            goal.status = GoalStatus::Failed;
                        }

                        ActionResult::failed(action_id, action.name(), e.to_string(), duration_ms)
                    }
                }
            }

            ExecutiveAction::DecomposeGoal {
                parent_goal_id,
                subgoals,
            } => {
                let duration_ms = start.elapsed().as_millis() as u64;
                let now = time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();

                for (idx, sub_def) in subgoals.iter().enumerate() {
                    let sub_id = format!("{parent_goal_id}.{}", idx + 1);
                    let mut criteria = vec![CompletionCriterion::SubGoalsCompleted];
                    for tool in &sub_def.required_tools {
                        criteria.push(CompletionCriterion::ToolExecuted {
                            tool_name: tool.clone(),
                        });
                    }

                    let goal = Goal {
                        id: sub_id,
                        description: sub_def.description.clone(),
                        parent_goal: Some(parent_goal_id.clone()),
                        priority: sub_def.priority,
                        status: GoalStatus::Pending,
                        dependencies: sub_def.dependencies.clone(),
                        preconditions: Vec::new(),
                        completion_criteria: criteria,
                        created_at: now.clone(),
                        completed_at: None,
                    };
                    let _ = state.goals.add_goal(goal);
                }

                ActionResult::succeeded(
                    action_id,
                    action.name(),
                    json!({"subgoals_created": subgoals.len()}),
                    duration_ms,
                )
            }

            ExecutiveAction::SynthesizeFindings { goal_ids: _ } => {
                let duration_ms = start.elapsed().as_millis() as u64;

                let all_tools: Vec<Value> = state
                    .knowledge_items
                    .values()
                    .filter(|k| k.item_type == KnowledgeItemType::Calculation)
                    .map(|k| k.content.clone())
                    .collect();

                let all_reports: Vec<Value> = state
                    .knowledge_items
                    .values()
                    .filter(|k| k.provenance.source_type == "specialist_report")
                    .map(|k| k.content.clone())
                    .collect();

                let synth_res: Result<crate::contracts::Synthesis, _> =
                    provider::generate_structured(
                        self.provider.as_ref(),
                        GenerationRequest {
                            stage: GenerationStage::Synthesis,
                            specialist: None,
                            system_instruction: "Synthesize specialist reports and computed tool outputs. Clearly separate mock output, hypotheses, calculations, evidence, assumptions, disagreements, uncertainty, and next steps. Return fields executive_summary,key_findings,disagreements,assumptions,uncertainties,next_steps.".into(),
                            input: json!({
                                "question": state.context.primary_objective,
                                "specialist_reports": all_reports,
                                "tools": all_tools,
                            }),
                        },
                        self.budget.as_ref(),
                    )
                    .await;

                match synth_res {
                    Ok(mut synthesis) => {
                        // Enforce epistemic integrity: models cannot assign verified
                        for claim in &mut synthesis.key_findings {
                            let verified = claim.tool_ids.iter().any(|id| {
                                state.knowledge_items.values().any(|k| {
                                    &k.id == id && k.epistemic_status == EpistemicStatus::Verified
                                })
                            });
                            let calculated = claim.tool_ids.iter().any(|id| {
                                state.knowledge_items.values().any(|k| {
                                    &k.id == id && k.epistemic_status == EpistemicStatus::Calculated
                                })
                            });

                            if claim.epistemic_status == EpistemicStatus::Verified && !verified {
                                claim.epistemic_status = if calculated {
                                    EpistemicStatus::Calculated
                                } else {
                                    EpistemicStatus::Hypothesis
                                };
                            }
                        }

                        let synth_val = serde_json::to_value(&synthesis).unwrap_or_default();
                        let k_item = KnowledgeItem {
                            id: format!("synthesis-{}", Uuid::new_v4()),
                            item_type: KnowledgeItemType::Fact,
                            content: synth_val.clone(),
                            provenance: Provenance::new("synthesis"),
                            epistemic_status: EpistemicStatus::Calculated,
                            related_goals: vec![],
                            related_entities: vec![],
                            timestamp: time::OffsetDateTime::now_utc()
                                .format(&time::format_description::well_known::Rfc3339)
                                .unwrap_or_default(),
                        };
                        state.add_knowledge_item(k_item);
                        state.context.current_stage = "completed".into();

                        ActionResult::succeeded(action_id, action.name(), synth_val, duration_ms)
                    }
                    Err(e) => {
                        ActionResult::failed(action_id, action.name(), e.to_string(), duration_ms)
                    }
                }
            }

            ExecutiveAction::UpdateCognitiveState { update_summary } => {
                let duration_ms = start.elapsed().as_millis() as u64;
                state.context.current_stage = update_summary.clone();
                ActionResult::succeeded(
                    action_id,
                    action.name(),
                    json!({"new_stage": update_summary}),
                    duration_ms,
                )
            }

            ExecutiveAction::StopInvestigation { reason } => {
                let duration_ms = start.elapsed().as_millis() as u64;
                state.executive.stop_reason = Some(reason.clone());
                state.executive.current_phase = crate::cognition::state::ExecutivePhase::Stopped;
                ActionResult::succeeded(
                    action_id,
                    action.name(),
                    json!({"stop_reason": reason.as_str()}),
                    duration_ms,
                )
            }

            ExecutiveAction::CompareHypotheses { hypothesis_ids } => {
                let duration_ms = start.elapsed().as_millis() as u64;
                let mut comparisons = Vec::new();
                for hid in hypothesis_ids {
                    if let Some(h) = state.hypotheses.get(hid) {
                        comparisons.push(json!({
                            "id": h.id,
                            "statement": h.statement,
                            "supporting_count": h.supporting_observations.len(),
                            "contradicting_count": h.contradicting_observations.len(),
                            "epistemic_status": h.epistemic_status.as_str(),
                        }));
                    }
                }
                ActionResult::succeeded(
                    action_id,
                    action.name(),
                    json!({"comparisons": comparisons}),
                    duration_ms,
                )
            }

            // Explicitly unsupported actions: NEVER report success!
            ExecutiveAction::RetrieveKnowledge { query, domain } => {
                let duration_ms = start.elapsed().as_millis() as u64;
                ActionResult::unsupported(
                    action_id,
                    action.name(),
                    format!(
                        "Scholarly knowledge retrieval for query '{query}' in domain '{domain}' is not supported in M1.3.1 (scheduled for M1.4)"
                    ),
                    duration_ms,
                )
            }

            ExecutiveAction::ClarifyMissingInformation {
                target_variable,
                reason,
            } => {
                let duration_ms = start.elapsed().as_millis() as u64;
                ActionResult::unsupported(
                    action_id,
                    action.name(),
                    format!(
                        "Automated clarification for '{target_variable}' ({reason}) is not supported"
                    ),
                    duration_ms,
                )
            }

            ExecutiveAction::RequestHumanInput { prompt } => {
                let duration_ms = start.elapsed().as_millis() as u64;
                ActionResult::new(
                    action_id,
                    action.name(),
                    ActionExecutionStatus::AwaitingHumanInput,
                    json!({"prompt": prompt}),
                    None,
                    duration_ms,
                )
            }
        }
    }
}
