use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Specialist {
    Mathematics,
    Physics,
    Chemistry,
    Biology,
    MedicinalScience,
}

impl Specialist {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Mathematics => "mathematics",
            Self::Physics => "physics",
            Self::Chemistry => "chemistry",
            Self::Biology => "biology",
            Self::MedicinalScience => "medicinal_science",
        }
    }

    pub fn allowed_tools(&self) -> &'static [&'static str] {
        match self {
            Self::Mathematics => &[
                "arithmetic",
                "symbolic",
                "power_law",
                "polynomial",
                "statistics",
                "linear_algebra",
                "ode_solve",
            ],
            Self::Physics => &[
                "arithmetic",
                "power_law",
                "projectile",
                "ode_solve",
                "statistics",
            ],
            Self::Chemistry => &["arithmetic", "statistics", "symbolic"],
            Self::Biology => &["statistics", "power_law", "linear_algebra"],
            Self::MedicinalScience => &["arithmetic", "statistics", "power_law"],
        }
    }

    pub fn instructions(&self) -> &'static str {
        match self {
            Self::Mathematics => {
                "Analyze equations, algebra, calculus, differential equations, statistics, numerical methods, and mathematical models. Check units and assumptions. Propose only tools from your allowlist. Give concise conclusions, not hidden chain-of-thought."
            }
            Self::Physics => {
                "Analyze physical principles, dimensional consistency, mathematical models, numerical assumptions, and physical limits. Distinguish idealized simulation from observation. Propose only tools from your allowlist. Give concise conclusions, not hidden chain-of-thought."
            }
            Self::Chemistry => {
                "Analyze chemical concepts, molecular and reaction reasoning, and supported chemical calculations. State when molecular data or experimental evidence is unavailable. Propose only tools from your allowlist. Give concise conclusions, not hidden chain-of-thought."
            }
            Self::Biology => {
                "Analyze biological mechanisms, cellular processes, genetics, molecular biology, and biological data. Separate established evidence from interpretation and hypothesis. Propose only tools from your allowlist. Give concise conclusions, not hidden chain-of-thought."
            }
            Self::MedicinalScience => {
                "Analyze medicinal chemistry, pharmacological mechanisms, drug-discovery research, and translational limits. Never imply clinical efficacy or safety from a computation alone. Propose only tools from your allowlist. Give concise conclusions, not hidden chain-of-thought."
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvestigationStatus {
    Planning,
    Executing,
    Synthesizing,
    Completed,
    Failed,
}

impl InvestigationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::Executing => "executing",
            Self::Synthesizing => "synthesizing",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicStatus {
    Verified,
    EvidenceSupported,
    Calculated,
    Hypothesis,
    Uncertain,
    Contradicted,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationPlan {
    pub objective: String,
    pub entities: Vec<String>,
    pub known_variables: Vec<String>,
    pub unknown_variables: Vec<String>,
    pub constraints: Vec<String>,
    pub expected_output: String,
    pub subproblems: Vec<ResearchSubproblem>,
    pub assumptions: Vec<String>,
    pub uncertainties: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchSubproblem {
    pub id: String,
    pub specialist: Specialist,
    pub task: String,
    pub depends_on: Vec<String>,
    pub required_tools: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpecialistOutput {
    pub analysis: String,
    pub claims: Vec<Claim>,
    pub tool_calls: Vec<ToolCall>,
    pub assumptions: Vec<String>,
    pub uncertainties: Vec<String>,
    pub evidence_requirements: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExecutionStatus {
    Completed,
    Mock,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentReport {
    pub specialist: Specialist,
    pub status: AgentExecutionStatus,
    pub analysis: String,
    pub claims: Vec<Claim>,
    pub tool_calls: Vec<ToolCall>,
    pub assumptions: Vec<String>,
    pub uncertainties: Vec<String>,
    pub evidence_requirements: Vec<String>,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub id: String,
    pub statement: String,
    pub topic: String,
    pub position: Option<String>,
    pub epistemic_status: EpistemicStatus,
    pub source_ids: Vec<String>,
    pub tool_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub tool: String,
    pub arguments: Value,
    pub rationale: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolExecution {
    pub id: String,
    pub specialist: Specialist,
    pub tool: String,
    pub arguments: Value,
    pub output: Option<Value>,
    pub method: Option<String>,
    pub epistemic_status: EpistemicStatus,
    pub verification_id: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VerificationResult {
    pub id: String,
    pub subject_id: String,
    pub status: EpistemicStatus,
    pub method: String,
    pub details: String,
    pub independently_checked: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Synthesis {
    pub executive_summary: String,
    pub key_findings: Vec<Claim>,
    pub disagreements: Vec<String>,
    pub assumptions: Vec<String>,
    pub uncertainties: Vec<String>,
    pub next_steps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelMetadata {
    pub provider: String,
    pub model: Option<String>,
    pub request_count: u32,
    pub http_attempt_count: u32,
    pub usage: Option<ModelUsage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Investigation {
    pub contract_version: String,
    pub id: String,
    pub question: String,
    pub created_at: String,
    pub updated_at: String,
    pub status: InvestigationStatus,
    pub mode: String,
    pub plan: Option<InvestigationPlan>,
    pub specialist_reports: Vec<AgentReport>,
    pub tools: Vec<ToolExecution>,
    pub verification: Vec<VerificationResult>,
    pub synthesis: Option<Synthesis>,
    pub model: ModelMetadata,
    pub sources: Vec<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationRequest {
    pub question: String,
}

#[derive(Debug, Deserialize)]
pub struct SettingsRequest {}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryRequest {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRequest {
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Serialize)]
pub struct HistoryItem {
    pub id: String,
    pub question: String,
    pub created_at: String,
    pub status: String,
    pub result: Value,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: &'static str,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub contract_version: &'static str,
    pub mode: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ProviderSettings {
    pub model_provider: String,
    pub model_name: Option<String>,
    pub memory_provider: &'static str,
    pub credentials_configured: bool,
    pub max_tokens_per_request: u32,
}
