use std::{ffi::OsString, path::PathBuf, time::Duration};

use axum::{
    Json, Router,
    extract::{Json as AxumJson, State, rejection::JsonRejection},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::{
    contracts::{
        ErrorResponse, HealthResponse, HistoryRequest, InvestigationRequest, SettingsRequest,
        ToolRequest,
    },
    orchestrator::Orchestrator,
    worker::{self, WorkerError},
};

const PAGE: &str = include_str!("../web/index.html");

#[derive(Clone)]
pub struct AppState {
    pub python: OsString,
    pub worker_script: PathBuf,
    pub worker_timeout: Duration,
    pub max_request_bytes: usize,
    pub orchestrator: std::sync::Arc<Orchestrator>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/api/health", get(health))
        .route("/api/investigate", post(investigate))
        .route("/api/history", post(history))
        .route(
            "/api/investigations/{id}",
            get(get_investigation).delete(delete_investigation),
        )
        .route("/api/tool", post(tool))
        .route("/api/settings", post(settings))
        .fallback(not_found)
        .layer(RequestBodyLimitLayer::new(state.max_request_bytes))
        .layer(middleware::from_fn(request_timeout))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn index() -> Html<&'static str> {
    Html(PAGE)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        contract_version: "1.0",
        mode: "offline-capable",
    })
}

async fn investigate(
    State(state): State<AppState>,
    request: Result<AxumJson<InvestigationRequest>, JsonRejection>,
) -> Response {
    let AxumJson(payload) = match request {
        Ok(payload) => payload,
        Err(error) => return json_rejection(error),
    };
    let question = payload.question.trim();
    if !(8..=4000).contains(&question.chars().count()) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_question",
            "Question must contain 8–4000 characters",
        );
    }
    match state.orchestrator.investigate(question.to_owned()).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(StatusCode::BAD_GATEWAY, "investigation_failed", &error),
    }
}

async fn history(
    State(state): State<AppState>,
    request: Result<AxumJson<HistoryRequest>, JsonRejection>,
) -> Response {
    let AxumJson(payload) = match request {
        Ok(payload) => payload,
        Err(error) => return json_rejection(error),
    };
    match state
        .orchestrator
        .store
        .list(payload.q, payload.limit.unwrap_or(50))
        .await
    {
        Ok(items) => Json(json!({"items":items})).into_response(),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            &error.to_string(),
        ),
    }
}

async fn get_investigation(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    match state.orchestrator.store.get(id).await {
        Ok(Some(item)) => Json(item.result).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "investigation_not_found",
            "Investigation was not found",
        ),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            &error.to_string(),
        ),
    }
}
async fn delete_investigation(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    match state.orchestrator.store.delete(id).await {
        Ok(true) => (StatusCode::NO_CONTENT, ()).into_response(),
        Ok(false) => error_response(
            StatusCode::NOT_FOUND,
            "investigation_not_found",
            "Investigation was not found",
        ),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            &error.to_string(),
        ),
    }
}

async fn tool(
    State(state): State<AppState>,
    request: Result<AxumJson<ToolRequest>, JsonRejection>,
) -> Response {
    let AxumJson(payload) = match request {
        Ok(payload) => payload,
        Err(error) => return json_rejection(error),
    };
    const ALLOWED_TOOLS: [&str; 5] = [
        "arithmetic",
        "power_law",
        "polynomial",
        "projectile",
        "statistics",
    ];
    if !ALLOWED_TOOLS.contains(&payload.name.as_str()) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "unsupported_tool",
            "The requested scientific tool is not available",
        );
    }
    dispatch(
        state,
        "/api/tool",
        json!({"name": payload.name, "args": payload.args}),
    )
    .await
}

async fn settings(
    State(state): State<AppState>,
    request: Result<AxumJson<SettingsRequest>, JsonRejection>,
) -> Response {
    if let Err(error) = request {
        return json_rejection(error);
    }
    let settings = state.orchestrator.provider.settings();
    Json(json!({"model_provider":settings.model_provider,"model_name":settings.model_name,"memory_provider":"sqlite","hosted_ready":settings.credentials_configured,"max_tokens_per_request":settings.max_tokens_per_request})).into_response()
}

async fn dispatch(state: AppState, route: &'static str, payload: Value) -> Response {
    match worker::invoke(
        state.python,
        state.worker_script,
        route,
        payload,
        state.worker_timeout,
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(error) => worker_error(error),
    }
}

fn worker_error(error: WorkerError) -> Response {
    let (status, code, message) = match error {
        WorkerError::Unavailable(message) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "worker_unavailable",
            format!("Scientific worker unavailable: {message}"),
        ),
        WorkerError::TimedOut => (
            StatusCode::GATEWAY_TIMEOUT,
            "worker_timeout",
            "Scientific worker exceeded its execution timeout".into(),
        ),
        WorkerError::Rejected(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
        WorkerError::Provider(message) => (StatusCode::BAD_GATEWAY, "provider_error", message),
        WorkerError::Failed(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "worker_failed",
            format!("Scientific worker failed: {message}"),
        ),
    };
    error_response(status, code, &message)
}

fn json_rejection(error: JsonRejection) -> Response {
    error_response(
        error.status(),
        "invalid_json",
        &format!("Invalid JSON request: {}", error.body_text()),
    )
}

fn error_response(status: StatusCode, code: &'static str, error: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: error.into(),
            code,
        }),
    )
        .into_response()
}

async fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "Route not found")
}

async fn request_timeout(request: Request<axum::body::Body>, next: Next) -> Response {
    match tokio::time::timeout(Duration::from_secs(300), next.run(request)).await {
        Ok(response) => response,
        Err(_) => error_response(
            StatusCode::GATEWAY_TIMEOUT,
            "request_timeout",
            "Request exceeded the server timeout",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{provider, store::Store};
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    fn state() -> AppState {
        AppState {
            python: "python".into(),
            worker_script: PathBuf::from("engine/app.py"),
            worker_timeout: Duration::from_secs(2),
            max_request_bytes: 262_144,
            orchestrator: {
                let path =
                    std::env::temp_dir().join(format!("ng-api-{}.sqlite3", uuid::Uuid::new_v4()));
                let (store, _) = Store::initialize(path).unwrap();
                let bundle = provider::from_environment().unwrap();
                std::sync::Arc::new(Orchestrator {
                    store,
                    provider: bundle.provider,
                    python: "python".into(),
                    worker_script: PathBuf::from("engine/app.py"),
                    worker_timeout: Duration::from_secs(2),
                })
            },
        }
    }

    async fn send(app: Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn health_reports_contract_version() {
        let response = router(state())
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["contract_version"], "1.0");
    }

    #[tokio::test]
    async fn rejects_malformed_json_as_json_error() {
        let request = Request::post("/api/investigate")
            .header("content-type", "application/json")
            .body(Body::from("{"))
            .unwrap();
        let (status, body) = send(router(state()), request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "invalid_json");
    }

    #[tokio::test]
    async fn rejects_oversized_body() {
        let mut limited = state();
        limited.max_request_bytes = 64;
        let request = Request::post("/api/investigate")
            .header("content-type", "application/json")
            .body(Body::from(format!(
                "{{\"question\":\"{}\"}}",
                "x".repeat(100)
            )))
            .unwrap();
        let response = router(limited).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn rejects_short_question_and_disallowed_tool() {
        let request = Request::post("/api/investigate")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"question":"tiny"}"#))
            .unwrap();
        let (status, body) = send(router(state()), request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "invalid_question");

        let request = Request::post("/api/tool")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"shell","args":{}}"#))
            .unwrap();
        let (status, body) = send(router(state()), request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "unsupported_tool");
    }

    #[tokio::test]
    async fn executes_allowlisted_tool_in_python_worker() {
        let request = Request::post("/api/tool")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"name":"arithmetic","args":{"a":20,"b":22}}"#,
            ))
            .unwrap();
        let (status, body) = send(router(state()), request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["value"], 42.0);
    }

    #[tokio::test]
    async fn unavailable_worker_is_reported() {
        let mut unavailable = state();
        unavailable.python = "neurogenesis-python-does-not-exist".into();
        let request = Request::post("/api/tool")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"arithmetic","args":{"a":1,"b":2}}"#))
            .unwrap();
        let (status, body) = send(router(unavailable), request).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["code"], "worker_unavailable");
    }

    #[tokio::test]
    async fn worker_timeout_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("slow.py");
        std::fs::write(&script, "import time; time.sleep(1)").unwrap();
        let mut slow = state();
        slow.worker_script = script;
        slow.worker_timeout = Duration::from_millis(20);
        let request = Request::post("/api/tool")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"arithmetic","args":{"a":1,"b":2}}"#))
            .unwrap();
        let (status, body) = send(router(slow), request).await;
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(body["code"], "worker_timeout");
    }
}
