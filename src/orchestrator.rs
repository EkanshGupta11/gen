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
    worker,
};

pub struct Orchestrator {
    pub store: Store,
    pub provider: Arc<dyn ModelProvider>,
    pub python: OsString,
    pub worker_script: PathBuf,
    pub worker_timeout: Duration,
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
        let budget = RequestBudget::new(self.provider.settings().max_tokens_per_request);
        let run: Result<Investigation, String> = async {
            let plan: InvestigationPlan = provider::generate_structured(self.provider.as_ref(), GenerationRequest{stage:GenerationStage::Planning,specialist:None,system_instruction:"Decompose the research question into a bounded structured plan. Return only the requested JSON schema. No executable instructions. Use concise objective, entities, variables, constraints, expected_output, subproblems [{id,specialist,task,depends_on,required_tools}], assumptions, uncertainties. Select only relevant specialists; at most five subproblems. Do not expose hidden reasoning.".into(),input:json!({"question":question})},&budget).await.map_err(|e|e.to_string())?;
            validate_plan(&plan)?;
            record.plan=Some(plan.clone()); record.status=InvestigationStatus::Executing; self.store.save(&record).await.map_err(|e|e.to_string())?;
            let mut seen=std::collections::HashSet::new();
            for sub in &plan.subproblems {
                if !seen.insert(sub.specialist.as_str()) { continue; }
                let start=Instant::now();
                let out: SpecialistOutput=provider::generate_structured(self.provider.as_ref(),GenerationRequest{stage:GenerationStage::Specialist,specialist:Some(sub.specialist.clone()),system_instruction:format!("{} Return JSON matching {{analysis,claims,tool_calls,assumptions,uncertainties,evidence_requirements}}. Claims must not be labelled verified or evidence_supported without independent proof or actual sources. Allowed tools: {:?}. Never issue code or shell commands.",sub.specialist.instructions(),sub.specialist.allowed_tools()),input:json!({"question":question,"task":sub.task,"plan":plan,"tool_results":record.tools})},&budget).await.map_err(|e|e.to_string())?;
                let mut report=AgentReport{specialist:sub.specialist.clone(),status:if self.provider.is_mock(){AgentExecutionStatus::Mock}else{AgentExecutionStatus::Completed},analysis:out.analysis,claims:out.claims,tool_calls:out.tool_calls.clone(),assumptions:out.assumptions,uncertainties:out.uncertainties,evidence_requirements:out.evidence_requirements,duration_ms:start.elapsed().as_millis() as u64};
                for (call_index, call) in out.tool_calls.into_iter().enumerate() {
                    if record.tools.len() >= 10 { break; }
                    if call_index >= 3 { record.tools.push(ToolExecution{id:Uuid::new_v4().to_string(),specialist:sub.specialist.clone(),tool:call.tool,arguments:call.arguments,output:None,method:None,epistemic_status:EpistemicStatus::Uncertain,verification_id:None,error:Some("Specialist tool-call limit reached (3 per role)".into()),duration_ms:0}); continue; }
                    if !sub.specialist.allowed_tools().contains(&call.tool.as_str()) { record.tools.push(ToolExecution{id:Uuid::new_v4().to_string(),specialist:sub.specialist.clone(),tool:call.tool,arguments:call.arguments,output:None,method:None,epistemic_status:EpistemicStatus::Uncertain,verification_id:None,error:Some("Tool rejected by the specialist allowlist".into()),duration_ms:0}); continue; }
                    let started=Instant::now();
                    let result=worker::invoke(self.python.clone(),self.worker_script.clone(),"/api/tool",json!({"name":call.tool,"args":call.arguments}),self.worker_timeout).await;
                    match result {
                        Ok(value)=> { let output=value.get("result").cloned().unwrap_or(value); let id=Uuid::new_v4().to_string(); record.verification.push(VerificationResult{id:id.clone(),subject_id:call.tool.clone(),status:EpistemicStatus::Calculated,method:"Deterministic local scientific computation; numerical result is not experimental validation".into(),details:"Tool output returned by the isolated Python calculation worker".into(),independently_checked:false}); record.tools.push(ToolExecution{id:Uuid::new_v4().to_string(),specialist:sub.specialist.clone(),tool:call.tool,arguments:call.arguments,method:output.get("method").and_then(Value::as_str).map(str::to_owned),output:Some(output),epistemic_status:EpistemicStatus::Calculated,verification_id:Some(id),error:None,duration_ms:started.elapsed().as_millis() as u64}); }
                        Err(e)=>record.tools.push(ToolExecution{id:Uuid::new_v4().to_string(),specialist:sub.specialist.clone(),tool:call.tool,arguments:call.arguments,output:None,method:None,epistemic_status:EpistemicStatus::Uncertain,verification_id:None,error:Some(format!("{e:?}")),duration_ms:started.elapsed().as_millis() as u64})
                    }
                }
                report.claims.iter_mut().for_each(|claim| { let backed=claim.source_ids.iter().any(|id|record.sources.iter().any(|source|source.get("id").and_then(Value::as_str)==Some(id))); let calculated=claim.tool_ids.iter().any(|id|record.tools.iter().any(|tool|&tool.id==id)); if !backed { claim.epistemic_status=if calculated {EpistemicStatus::Calculated} else {EpistemicStatus::Hypothesis}; } });
                record.specialist_reports.push(report); record.updated_at=timestamp(); self.store.save(&record).await.map_err(|e|e.to_string())?;
            }
            record.status=InvestigationStatus::Synthesizing;
            let mut synthesis:Synthesis=provider::generate_structured(self.provider.as_ref(),GenerationRequest{stage:GenerationStage::Synthesis,specialist:None,system_instruction:"Synthesize specialist reports and computed tool outputs. Clearly separate mock output, hypotheses, calculations, evidence, assumptions, disagreements, uncertainty, and next steps. Do not claim evidence from unavailable sources. Return fields executive_summary,key_findings,disagreements,assumptions,uncertainties,next_steps.".into(),input:json!({"question":question,"plan":plan,"specialist_reports":record.specialist_reports,"tools":record.tools,"sources":record.sources})},&budget).await.map_err(|e|e.to_string())?;
            for claim in &mut synthesis.key_findings { let backed=claim.source_ids.iter().any(|id|record.sources.iter().any(|source|source.get("id").and_then(Value::as_str)==Some(id))); let calculated=claim.tool_ids.iter().any(|id|record.tools.iter().any(|tool|&tool.id==id)); if !backed { claim.epistemic_status=if calculated {EpistemicStatus::Calculated} else {EpistemicStatus::Hypothesis}; } }
            record.synthesis=Some(synthesis); record.status=InvestigationStatus::Completed; record.updated_at=timestamp(); record.model.request_count=budget.model_call_count(); record.model.http_attempt_count=budget.http_attempt_count(); record.model.usage=budget.usage();
            self.store.save(&record).await.map_err(|e|e.to_string())?;
            Ok(record.clone())
        }.await;
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

    #[tokio::test]
    async fn offline_investigation_runs_plan_specialist_tool_synthesis_and_persists() {
        let directory = tempfile::tempdir().unwrap();
        let (store, _) =
            Store::initialize(directory.path().join("investigations.sqlite3")).unwrap();
        let python = if cfg!(windows) {
            if std::path::Path::new(".venv-cpython313/Scripts/python.exe").exists() {
                ".venv-cpython313/Scripts/python.exe"
            } else {
                ".venv/bin/python.exe"
            }
        } else {
            "python3"
        };
        let orchestration = Orchestrator {
            store,
            provider: Arc::new(MockProvider { max_tokens: 1200 }),
            python: python.into(),
            worker_script: PathBuf::from("engine/app.py"),
            worker_timeout: Duration::from_secs(5),
        };
        let record = orchestration.investigate("For a projectile launched at speed 20 m/s and angle 35 degrees, estimate ideal range.".into()).await.unwrap();
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
        assert!(
            (record.tools[0].output.as_ref().unwrap()["range_m"]
                .as_f64()
                .unwrap()
                - 38.33)
                .abs()
                < 0.02
        );
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
}
