use std::{ffi::OsString, path::PathBuf, time::Duration};

use serde_json::Value;
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};

#[derive(Debug)]
pub enum WorkerError {
    Unavailable(String),
    TimedOut,
    Rejected(String),
    Provider(String),
    Failed(String),
}

pub async fn invoke(
    python: OsString,
    script: PathBuf,
    route: &'static str,
    mut payload: Value,
    limit: Duration,
) -> Result<Value, WorkerError> {
    let run = async move {
        if let Some(object) = payload.as_object_mut() {
            object.insert("contract_version".into(), Value::String("1.0".into()));
        }
        let mut child = Command::new(python)
            .arg(script)
            .arg(route)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| WorkerError::Unavailable(error.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            let bytes = serde_json::to_vec(&payload)
                .map_err(|error| WorkerError::Failed(error.to_string()))?;
            stdin
                .write_all(&bytes)
                .await
                .map_err(|error| WorkerError::Failed(error.to_string()))?;
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|error| WorkerError::Failed(error.to_string()))?;
        let decoded: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
            WorkerError::Failed(format!("worker returned invalid JSON: {error}"))
        })?;
        if output.status.success()
            && decoded.get("contract_version").and_then(Value::as_str) != Some("1.0")
        {
            return Err(WorkerError::Failed(
                "worker contract version mismatch".into(),
            ));
        }
        if output.status.success() {
            return Ok(decoded);
        }
        let message = decoded
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Scientific worker rejected the request")
            .to_owned();
        match output.status.code() {
            Some(2) => Err(WorkerError::Rejected(message)),
            Some(4) => Err(WorkerError::Provider(message)),
            _ => Err(WorkerError::Failed(message)),
        }
    };

    timeout(limit, run)
        .await
        .map_err(|_| WorkerError::TimedOut)?
}
