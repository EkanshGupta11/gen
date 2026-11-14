use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct InvestigationRequest {
    pub question: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct HistoryRequest {
    #[serde(default)]
    pub q: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ToolRequest {
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Default, Deserialize)]
pub struct SettingsRequest {}

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
