use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use thiserror::Error;
use tokio::task;

use crate::contracts::{HistoryItem, Investigation};

const SCHEMA_VERSION: i64 = 2;

#[derive(Clone, Debug)]
pub struct Store {
    path: PathBuf,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("SQLite backup failed: {0}")]
    Backup(String),
    #[error("SQLite migration found an unsupported legacy schema")]
    UnsupportedSchema,
    #[error("SQLite background task failed: {0}")]
    Task(String),
    #[error("Stored investigation JSON is invalid")]
    InvalidStoredJson,
}

impl Store {
    pub fn initialize(path: impl Into<PathBuf>) -> Result<(Self, Option<PathBuf>), StoreError> {
        let store = Self { path: path.into() };
        if let Some(parent) = store.path.parent() {
            fs::create_dir_all(parent).map_err(|error| StoreError::Backup(error.to_string()))?;
        }
        let mut connection = open_connection(&store.path)?;
        let has_table = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='investigations')",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        let mut backup = None;
        if has_table && schema_version(&connection)? < SCHEMA_VERSION {
            backup = Some(create_migration_backup(&connection, &store.path)?);
        }
        migrate(&mut connection, has_table)?;
        Ok((store, backup))
    }

    pub async fn save(&self, investigation: &Investigation) -> Result<(), StoreError> {
        let path = self.path.clone();
        let record = investigation.clone();
        task::spawn_blocking(move || save_sync(&path, &record))
            .await
            .map_err(|error| StoreError::Task(error.to_string()))?
    }

    pub async fn get(&self, id: String) -> Result<Option<HistoryItem>, StoreError> {
        let path = self.path.clone();
        task::spawn_blocking(move || get_sync(&path, &id))
            .await
            .map_err(|error| StoreError::Task(error.to_string()))?
    }

    pub async fn list(
        &self,
        query: Option<String>,
        limit: usize,
    ) -> Result<Vec<HistoryItem>, StoreError> {
        let path = self.path.clone();
        task::spawn_blocking(move || list_sync(&path, query.as_deref(), limit.clamp(1, 100)))
            .await
            .map_err(|error| StoreError::Task(error.to_string()))?
    }

    pub async fn delete(&self, id: String) -> Result<bool, StoreError> {
        let path = self.path.clone();
        task::spawn_blocking(move || {
            let connection = open_connection(&path)?;
            Ok(connection.execute("DELETE FROM investigations WHERE id=?1", [id])? > 0)
        })
        .await
        .map_err(|error| StoreError::Task(error.to_string()))?
    }

    #[allow(dead_code)]
    pub async fn save_cognitive_state(
        &self,
        investigation_id: String,
        state: &crate::cognition::CognitiveState,
    ) -> Result<(), StoreError> {
        let path = self.path.clone();
        let state_json = serde_json::to_string(state).map_err(|_| StoreError::InvalidStoredJson)?;
        task::spawn_blocking(move || {
            let connection = open_connection(&path)?;
            connection.execute(
                "UPDATE investigations SET cognitive_state_json=?1 WHERE id=?2",
                params![state_json, investigation_id],
            )?;
            Ok(())
        })
        .await
        .map_err(|error| StoreError::Task(error.to_string()))?
    }

    #[allow(dead_code)]
    pub async fn get_cognitive_state(
        &self,
        investigation_id: String,
    ) -> Result<Option<crate::cognition::CognitiveState>, StoreError> {
        let path = self.path.clone();
        task::spawn_blocking(move || {
            let connection = open_connection(&path)?;
            let raw: Option<String> = connection
                .query_row(
                    "SELECT cognitive_state_json FROM investigations WHERE id=?1",
                    params![investigation_id],
                    |row| row.get(0),
                )
                .optional()?;
            match raw {
                Some(json_str) => {
                    let state = serde_json::from_str(&json_str)
                        .map_err(|_| StoreError::InvalidStoredJson)?;
                    Ok(Some(state))
                }
                None => Ok(None),
            }
        })
        .await
        .map_err(|error| StoreError::Task(error.to_string()))?
    }

    #[allow(dead_code)]
    pub async fn append_cognitive_events(
        &self,
        events: Vec<crate::cognition::CognitiveEvent>,
    ) -> Result<(), StoreError> {
        let path = self.path.clone();
        task::spawn_blocking(move || {
            let mut connection = open_connection(&path)?;
            let tx = connection.transaction()?;
            for ev in events {
                let payload = serde_json::to_string(&ev.payload)
                    .map_err(|_| StoreError::InvalidStoredJson)?;
                tx.execute(
                    "INSERT INTO cognitive_events (id, investigation_id, cycle, timestamp, event_type, payload_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(id) DO NOTHING",
                    params![
                        ev.id,
                        ev.investigation_id,
                        ev.cycle,
                        ev.timestamp,
                        ev.event_type.as_str(),
                        payload
                    ],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
        .map_err(|error| StoreError::Task(error.to_string()))?
    }

    #[allow(dead_code)]
    pub async fn get_cognitive_events(
        &self,
        investigation_id: String,
    ) -> Result<Vec<crate::cognition::CognitiveEvent>, StoreError> {
        let path = self.path.clone();
        task::spawn_blocking(move || {
            let connection = open_connection(&path)?;
            let mut stmt = connection.prepare(
                "SELECT id, investigation_id, cycle, timestamp, event_type, payload_json
                 FROM cognitive_events
                 WHERE investigation_id=?1
                 ORDER BY cycle ASC, timestamp ASC",
            )?;
            let rows = stmt.query_map(params![investigation_id], |row| {
                let id: String = row.get(0)?;
                let inv_id: String = row.get(1)?;
                let cycle: u32 = row.get(2)?;
                let timestamp: String = row.get(3)?;
                let event_type_str: String = row.get(4)?;
                let payload_str: String = row.get(5)?;
                Ok((id, inv_id, cycle, timestamp, event_type_str, payload_str))
            })?;

            let mut events = Vec::new();
            for r in rows {
                let (id, inv_id, cycle, timestamp, event_type_str, payload_str) = r?;
                let payload: Value = serde_json::from_str(&payload_str)
                    .map_err(|_| StoreError::InvalidStoredJson)?;
                let event_type = match event_type_str.as_str() {
                    "cycle_started" => crate::cognition::CognitiveEventType::CycleStarted,
                    "state_updated" => crate::cognition::CognitiveEventType::StateUpdated,
                    "goal_created" => crate::cognition::CognitiveEventType::GoalCreated,
                    "goal_status_changed" => {
                        crate::cognition::CognitiveEventType::GoalStatusChanged
                    }
                    "memory_item_inserted" => {
                        crate::cognition::CognitiveEventType::MemoryItemInserted
                    }
                    "memory_item_updated" => {
                        crate::cognition::CognitiveEventType::MemoryItemUpdated
                    }
                    "memory_item_evicted" => {
                        crate::cognition::CognitiveEventType::MemoryItemEvicted
                    }
                    "attention_focused" => crate::cognition::CognitiveEventType::AttentionFocused,
                    "action_selected" => crate::cognition::CognitiveEventType::ActionSelected,
                    "action_precondition_validated" => {
                        crate::cognition::CognitiveEventType::ActionPreconditionValidated
                    }
                    "action_precondition_failed" => {
                        crate::cognition::CognitiveEventType::ActionPreconditionFailed
                    }
                    "action_executed" => crate::cognition::CognitiveEventType::ActionExecuted,
                    "action_evaluation" => crate::cognition::CognitiveEventType::ActionEvaluation,
                    "metacognitive_evaluation" => {
                        crate::cognition::CognitiveEventType::MetacognitiveEvaluation
                    }
                    "investigation_stopped" => {
                        crate::cognition::CognitiveEventType::InvestigationStopped
                    }
                    _ => crate::cognition::CognitiveEventType::StateUpdated,
                };
                events.push(crate::cognition::CognitiveEvent {
                    id,
                    investigation_id: inv_id,
                    cycle,
                    timestamp,
                    event_type,
                    payload,
                });
            }
            Ok(events)
        })
        .await
        .map_err(|error| StoreError::Task(error.to_string()))?
    }
}

fn open_connection(path: &Path) -> Result<Connection, rusqlite::Error> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    Ok(connection)
}

fn schema_version(connection: &Connection) -> Result<i64, rusqlite::Error> {
    connection.pragma_query_value(None, "user_version", |row| row.get(0))
}

fn create_migration_backup(connection: &Connection, path: &Path) -> Result<PathBuf, StoreError> {
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("neurogenesis");
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut index = 0_u32;
    loop {
        let suffix = if index == 0 {
            String::new()
        } else {
            format!("-{index:03}")
        };
        let destination = parent.join(format!("{stem}.pre-m1.3{suffix}.bak.sqlite3"));
        if destination.exists() {
            index += 1;
            continue;
        }
        connection
            .backup("main", &destination, None::<fn(rusqlite::backup::Progress)>)
            .map_err(|error| StoreError::Backup(error.to_string()))?;
        return Ok(destination);
    }
}

fn migrate(connection: &mut Connection, has_table: bool) -> Result<(), StoreError> {
    if !has_table {
        connection.execute_batch(
            "CREATE TABLE investigations (
                id TEXT PRIMARY KEY NOT NULL,
                question TEXT NOT NULL,
                created_at TEXT NOT NULL,
                result TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'planning',
                plan_json TEXT,
                specialist_reports_json TEXT,
                tool_results_json TEXT,
                verification_json TEXT,
                synthesis_json TEXT,
                model_metadata_json TEXT,
                error_json TEXT,
                cognitive_state_json TEXT
            );
            CREATE INDEX investigations_created_at ON investigations(created_at DESC);
            CREATE TABLE IF NOT EXISTS cognitive_events (
                id TEXT PRIMARY KEY NOT NULL,
                investigation_id TEXT NOT NULL,
                cycle INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_cognitive_events_inv ON cognitive_events(investigation_id, cycle, timestamp);
            PRAGMA user_version=2;",
        )?;
        return Ok(());
    }

    let existing = column_names(connection)?;
    for required in ["id", "question", "created_at", "result"] {
        if !existing.iter().any(|column| column == required) {
            return Err(StoreError::UnsupportedSchema);
        }
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let migrations = [
        (
            "updated_at",
            "ALTER TABLE investigations ADD COLUMN updated_at TEXT",
        ),
        (
            "status",
            "ALTER TABLE investigations ADD COLUMN status TEXT NOT NULL DEFAULT 'completed'",
        ),
        (
            "plan_json",
            "ALTER TABLE investigations ADD COLUMN plan_json TEXT",
        ),
        (
            "specialist_reports_json",
            "ALTER TABLE investigations ADD COLUMN specialist_reports_json TEXT",
        ),
        (
            "tool_results_json",
            "ALTER TABLE investigations ADD COLUMN tool_results_json TEXT",
        ),
        (
            "verification_json",
            "ALTER TABLE investigations ADD COLUMN verification_json TEXT",
        ),
        (
            "synthesis_json",
            "ALTER TABLE investigations ADD COLUMN synthesis_json TEXT",
        ),
        (
            "model_metadata_json",
            "ALTER TABLE investigations ADD COLUMN model_metadata_json TEXT",
        ),
        (
            "error_json",
            "ALTER TABLE investigations ADD COLUMN error_json TEXT",
        ),
        (
            "cognitive_state_json",
            "ALTER TABLE investigations ADD COLUMN cognitive_state_json TEXT",
        ),
    ];
    for (column, statement) in migrations {
        if !existing
            .iter()
            .any(|existing_column| existing_column == column)
        {
            transaction.execute_batch(statement)?;
        }
    }
    transaction.execute(
        "UPDATE investigations SET updated_at=created_at WHERE updated_at IS NULL",
        [],
    )?;
    transaction.execute_batch(
        "CREATE INDEX IF NOT EXISTS investigations_created_at ON investigations(created_at DESC);
         CREATE TABLE IF NOT EXISTS cognitive_events (
             id TEXT PRIMARY KEY NOT NULL,
             investigation_id TEXT NOT NULL,
             cycle INTEGER NOT NULL,
             timestamp TEXT NOT NULL,
             event_type TEXT NOT NULL,
             payload_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_cognitive_events_inv ON cognitive_events(investigation_id, cycle, timestamp);
         PRAGMA user_version=2;",
    )?;
    transaction.commit()?;
    Ok(())
}

fn column_names(connection: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut statement = connection.prepare("PRAGMA table_info(investigations)")?;
    let rows = statement.query_map([], |row| row.get(1))?;
    rows.collect()
}

fn save_sync(path: &Path, investigation: &Investigation) -> Result<(), StoreError> {
    let connection = open_connection(path)?;
    let full = serde_json::to_string(investigation).map_err(|_| StoreError::InvalidStoredJson)?;
    let plan = json_option(&investigation.plan)?;
    let reports = serde_json::to_string(&investigation.specialist_reports)
        .map_err(|_| StoreError::InvalidStoredJson)?;
    let tools =
        serde_json::to_string(&investigation.tools).map_err(|_| StoreError::InvalidStoredJson)?;
    let verification = serde_json::to_string(&investigation.verification)
        .map_err(|_| StoreError::InvalidStoredJson)?;
    let synthesis = json_option(&investigation.synthesis)?;
    let model =
        serde_json::to_string(&investigation.model).map_err(|_| StoreError::InvalidStoredJson)?;
    let error = json_option(&investigation.error)?;
    let cognitive_state = json_option(&investigation.cognitive_state)?;
    connection.execute(
        "INSERT INTO investigations (id,question,created_at,result,updated_at,status,plan_json,specialist_reports_json,tool_results_json,verification_json,synthesis_json,model_metadata_json,error_json,cognitive_state_json)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
         ON CONFLICT(id) DO UPDATE SET question=excluded.question, result=excluded.result, updated_at=excluded.updated_at, status=excluded.status, plan_json=excluded.plan_json, specialist_reports_json=excluded.specialist_reports_json, tool_results_json=excluded.tool_results_json, verification_json=excluded.verification_json, synthesis_json=excluded.synthesis_json, model_metadata_json=excluded.model_metadata_json, error_json=excluded.error_json, cognitive_state_json=excluded.cognitive_state_json",
        params![
            investigation.id,
            investigation.question,
            investigation.created_at,
            full,
            investigation.updated_at,
            investigation.status.as_str(),
            plan,
            reports,
            tools,
            verification,
            synthesis,
            model,
            error,
            cognitive_state,
        ],
    )?;
    Ok(())
}

fn json_option<T: serde::Serialize>(value: &Option<T>) -> Result<Option<String>, StoreError> {
    value
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| StoreError::InvalidStoredJson)
}

fn get_sync(path: &Path, id: &str) -> Result<Option<HistoryItem>, StoreError> {
    let connection = open_connection(path)?;
    let row = connection
        .query_row(
            "SELECT id,question,created_at,status,result FROM investigations WHERE id=?1",
            [id],
            row_to_item,
        )
        .optional()?;
    Ok(row)
}

fn list_sync(
    path: &Path,
    query: Option<&str>,
    limit: usize,
) -> Result<Vec<HistoryItem>, StoreError> {
    let connection = open_connection(path)?;
    let mut statement = connection.prepare(
        "SELECT id,question,created_at,status,result FROM investigations
         WHERE (?1 IS NULL OR instr(lower(question),lower(?1)) > 0)
         ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = statement.query_map(params![query, limit as i64], row_to_item)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

fn row_to_item(row: &rusqlite::Row<'_>) -> Result<HistoryItem, rusqlite::Error> {
    let result_text: String = row.get(4)?;
    let result: Value = serde_json::from_str(&result_text).unwrap_or(Value::String(result_text));
    Ok(HistoryItem {
        id: row.get(0)?,
        question: row.get(1)?,
        created_at: row.get(2)?,
        status: row.get(3)?,
        result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{Investigation, InvestigationStatus, ModelMetadata, Specialist};

    fn sample_investigation(id: &str) -> Investigation {
        Investigation {
            contract_version: "1.0".into(),
            id: id.into(),
            question: "A test scientific question".into(),
            created_at: "2026-11-14T00:00:00Z".into(),
            updated_at: "2026-11-14T00:00:00Z".into(),
            status: InvestigationStatus::Completed,
            mode: "mock_offline".into(),
            plan: None,
            specialist_reports: vec![],
            tools: vec![],
            verification: vec![],
            synthesis: None,
            model: ModelMetadata {
                provider: "mock".into(),
                model: Some("offline-mock".into()),
                request_count: 0,
                http_attempt_count: 0,
                usage: None,
            },
            sources: vec![],
            error: None,
            cognitive_state: None,
        }
    }

    #[tokio::test]
    async fn migrates_legacy_schema_with_backup_and_preserves_rows() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("legacy.sqlite3");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE investigations(id TEXT PRIMARY KEY, question TEXT, created_at TEXT, result TEXT);").unwrap();
        connection
            .execute(
                "INSERT INTO investigations VALUES (?1,?2,?3,?4)",
                params![
                    "legacy-1",
                    "Existing question",
                    "2026-01-01T00:00:00Z",
                    "{\"old\":true}"
                ],
            )
            .unwrap();
        drop(connection);

        let (store, backup) = Store::initialize(&database).unwrap();
        let backup = backup.unwrap();
        assert!(backup.exists());
        let backup_connection = Connection::open(&backup).unwrap();
        assert_eq!(
            backup_connection
                .query_row("SELECT COUNT(*) FROM investigations", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let migrated = store.get("legacy-1".into()).await.unwrap().unwrap();
        assert_eq!(migrated.question, "Existing question");
        assert_eq!(migrated.status, "completed");
        assert_eq!(migrated.result["old"], true);
        assert!(migrated.result["plan"].is_null());
    }

    #[tokio::test]
    async fn persists_updates_and_retrieves_from_a_fresh_store_instance() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.sqlite3");
        let (store, backup) = Store::initialize(&path).unwrap();
        assert!(backup.is_none());
        let mut record = sample_investigation("id-1");
        record
            .specialist_reports
            .push(crate::contracts::AgentReport {
                specialist: Specialist::Mathematics,
                status: crate::contracts::AgentExecutionStatus::Mock,
                analysis: "mock only".into(),
                claims: vec![],
                tool_calls: vec![],
                assumptions: vec![],
                uncertainties: vec![],
                evidence_requirements: vec![],
                duration_ms: 0,
            });
        store.save(&record).await.unwrap();
        drop(store);

        let (reopened, _) = Store::initialize(&path).unwrap();
        let history = reopened.list(Some("scientific".into()), 10).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].result["specialist_reports"][0]["status"], "mock");
        assert!(reopened.delete("id-1".into()).await.unwrap());
        assert!(reopened.get("id-1".into()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn cognitive_state_persistence_and_resumption_across_store_instances() {
        use std::time::Duration;

        use crate::cognition::*;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cognition_resume.sqlite3");

        // 1. Creating cognitive state
        let inv_id = "inv-resume-test".to_string();
        let mut controller = ExecutiveController::new(
            inv_id.clone(),
            "Calculate and verify trajectory",
            ExecutiveConfig {
                max_cycles: 20,
                max_actions: 20,
                timeout: Duration::from_secs(10),
                working_memory_capacity: 10,
            },
        );

        // Add 2 sequential goals
        let goal1 = Goal {
            id: "step-1".into(),
            description: "Execute projectile calculation".into(),
            parent_goal: None,
            priority: 90,
            status: GoalStatus::Active,
            dependencies: vec![],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::ToolExecuted {
                tool_name: "projectile".into(),
            }],
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: None,
        };
        let goal2 = Goal {
            id: "step-2".into(),
            description: "Update summary after step 1".into(),
            parent_goal: None,
            priority: 80,
            status: GoalStatus::Pending,
            dependencies: vec!["step-1".into()],
            preconditions: vec![],
            completion_criteria: vec![CompletionCriterion::SubGoalsCompleted],
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: None,
        };
        controller.state.goals.add_goal(goal1).unwrap();
        controller.state.goals.add_goal(goal2).unwrap();

        // Add an agenda action for step-2
        controller.state.goals.add_agenda_item(AgendaItem {
            id: "agenda-step-2".into(),
            goal_id: "step-2".into(),
            action: ExecutiveAction::UpdateCognitiveState {
                update_summary: "trajectory_verified".into(),
            },
            status: AgendaStatus::Blocked,
            created_at: "2026-01-01T00:00:00Z".into(),
        });

        // 2. Updating working memory & 3. Executing controller action (which completes goal 1)
        let cont = controller.step().unwrap();
        assert!(cont);
        // Step again to evaluate completion criteria for step-1
        let cont2 = controller.step().unwrap();
        assert!(cont2);

        assert_eq!(
            controller.state.goals.get_goal("step-1").unwrap().status,
            GoalStatus::Completed,
            "Goal step-1 must be completed"
        );
        assert_eq!(
            controller.state.goals.get_goal("step-2").unwrap().status,
            GoalStatus::Active,
            "Goal step-2 must now be Active because step-1 completed"
        );
        assert_eq!(
            controller.state.working_memory.len(),
            1,
            "Working memory must contain the projectile calculation item"
        );

        // 4. Persisting state & events to SQLite store
        let (store, _) = Store::initialize(&path).unwrap();
        let sample_record = Investigation {
            contract_version: "1.0".into(),
            id: inv_id.clone(),
            question: "Calculate and verify trajectory".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            status: crate::contracts::InvestigationStatus::Executing,
            mode: "mock_offline".into(),
            plan: None,
            specialist_reports: vec![],
            tools: vec![],
            verification: vec![],
            synthesis: None,
            model: crate::contracts::ModelMetadata {
                provider: "mock".into(),
                model: None,
                request_count: 0,
                http_attempt_count: 0,
                usage: None,
            },
            sources: vec![],
            error: None,
            cognitive_state: Some(controller.state.clone()),
        };
        store.save(&sample_record).await.unwrap();
        store
            .save_cognitive_state(inv_id.clone(), &controller.state)
            .await
            .unwrap();
        store
            .append_cognitive_events(controller.events.clone())
            .await
            .unwrap();

        // 5. Stopping the application (dropping store and controller)
        drop(controller);
        drop(store);

        // 6. Restarting it: open fresh store instance from disk
        let (reopened_store, _) = Store::initialize(&path).unwrap();

        // 7. Restoring the investigation's cognitive state
        let restored_state = reopened_store
            .get_cognitive_state(inv_id.clone())
            .await
            .unwrap()
            .expect("Cognitive state must be recovered from SQLite");

        assert_eq!(restored_state.context.investigation_id, inv_id);
        assert_eq!(
            restored_state.goals.get_goal("step-1").unwrap().status,
            GoalStatus::Completed
        );
        assert_eq!(
            restored_state.goals.get_goal("step-2").unwrap().status,
            GoalStatus::Active
        );
        assert_eq!(restored_state.working_memory.len(), 1);

        // Check that cognitive events were also restored
        let restored_events = reopened_store
            .get_cognitive_events(inv_id.clone())
            .await
            .unwrap();
        assert!(!restored_events.is_empty());

        // 8. Continuing with the next eligible action on resumed controller
        let mut resumed_controller = ExecutiveController::from_existing_state(
            restored_state,
            ExecutiveConfig {
                max_cycles: 10,
                max_actions: 10,
                timeout: Duration::from_secs(5),
                working_memory_capacity: 10,
            },
        );

        // Step the resumed controller: must execute step-2 action, NOT repeat step-1!
        let resumed_cont = resumed_controller.step().unwrap();
        assert!(resumed_cont);

        let last_action = resumed_controller
            .state
            .executive
            .selected_action
            .as_ref()
            .unwrap();
        assert_eq!(
            last_action.name(),
            "update_cognitive_state",
            "Resumed controller must continue with next eligible action without repeating completed step-1"
        );
        assert_eq!(
            resumed_controller.state.context.current_stage,
            "trajectory_verified"
        );
    }
}
