use crate::{
    cognition::{
        actions::{ActionResult, ExecutiveAction},
        executive::{ExecutiveConfig, ExecutiveController},
        executor::AsyncActionExecutor,
        state::{CognitiveState, ExecutivePhase},
    },
    store::Store,
};

pub struct CognitiveRuntime {
    pub controller: ExecutiveController,
    pub executor: AsyncActionExecutor,
    pub store: Store,
    investigation_id: String,
}

impl CognitiveRuntime {
    pub fn new(
        investigation_id: impl Into<String>,
        primary_objective: impl Into<String>,
        config: ExecutiveConfig,
        executor: AsyncActionExecutor,
        store: Store,
    ) -> Self {
        let inv_id = investigation_id.into();
        let controller = ExecutiveController::new(inv_id.clone(), primary_objective, config);
        Self {
            controller,
            executor,
            store,
            investigation_id: inv_id,
        }
    }

    pub fn from_existing_state(
        state: CognitiveState,
        config: ExecutiveConfig,
        executor: AsyncActionExecutor,
        store: Store,
    ) -> Self {
        let inv_id = state.context.investigation_id.clone();
        let controller = ExecutiveController::from_existing_state(state, config);
        Self {
            controller,
            executor,
            store,
            investigation_id: inv_id,
        }
    }

    /// Resume an existing investigation from persistent storage.
    pub async fn resume(
        investigation_id: &str,
        config: ExecutiveConfig,
        executor: AsyncActionExecutor,
        store: Store,
    ) -> Result<Option<Self>, String> {
        let state_opt = store
            .get_cognitive_state(investigation_id.to_string())
            .await
            .map_err(|e| e.to_string())?;

        match state_opt {
            Some(state) => Ok(Some(Self::from_existing_state(
                state, config, executor, store,
            ))),
            None => Ok(None),
        }
    }

    /// Execute a single discrete cognitive runtime cycle asynchronously:
    /// Decision (Controller) -> Async Execution (Executor) -> Outcome Commit (Controller) -> Persistence (Store).
    pub async fn step(&mut self) -> Result<bool, String> {
        if self.controller.state.is_stopped() {
            self.persist_state_and_events().await?;
            return Ok(false);
        }

        // Authoritative executive control lifecycle: prepares single action or concurrent batch
        let batch = self
            .controller
            .step_prepare_batch(self.controller.config.max_concurrency)?;

        if batch.is_empty() {
            self.persist_state_and_events().await?;
            return Ok(!self.controller.state.is_stopped());
        }

        if batch.len() == 1 {
            let action = batch.into_iter().next().unwrap();
            self.controller.state.executive.current_phase = ExecutivePhase::ExecuteAction;
            let outcome = self
                .executor
                .execute(&action, &mut self.controller.state)
                .await;
            self.controller.step_commit(&action, outcome)?;
            self.persist_state_and_events().await?;
            return Ok(!self.controller.state.is_stopped());
        }

        // Bounded concurrent execution across worker tasks with deterministic batch commit order
        self.controller.state.executive.current_phase = ExecutivePhase::ExecuteAction;
        let batch_len = batch.len();
        let batch_actions = batch.clone();
        let mut join_set = tokio::task::JoinSet::new();
        let mut task_id_to_idx = std::collections::HashMap::new();

        for (batch_idx, action) in batch.into_iter().enumerate() {
            let exec = self.executor.clone();
            let act = action.clone();
            let mut state_clone = self.controller.state.clone();
            let handle = join_set.spawn(async move {
                let outcome = exec.execute(&act, &mut state_clone).await;
                (batch_idx, act, outcome, state_clone)
            });
            task_id_to_idx.insert(handle.id(), batch_idx);
        }

        // Collect concurrent execution results indexed by original batch position
        let mut collected: Vec<Option<(ExecutiveAction, ActionResult, Option<CognitiveState>)>> =
            (0..batch_len).map(|_| None).collect();

        while let Some(res) = join_set.join_next().await {
            match res {
                Ok((batch_idx, act, outcome, task_state)) => {
                    collected[batch_idx] = Some((act, outcome, Some(task_state)));
                }
                Err(join_err) => {
                    let task_id = join_err.id();
                    let failed_idx = task_id_to_idx
                        .get(&task_id)
                        .copied()
                        .or_else(|| collected.iter().position(|slot| slot.is_none()));

                    if let Some(idx) = failed_idx {
                        let act = batch_actions[idx].clone();
                        let err_msg = if join_err.is_panic() {
                            format!(
                                "Concurrent task at batch index {idx} panicked during execution"
                            )
                        } else if join_err.is_cancelled() {
                            format!("Concurrent task at batch index {idx} was cancelled")
                        } else {
                            format!("Concurrent task at batch index {idx} failed: {join_err}")
                        };
                        let outcome = ActionResult::failed(
                            uuid::Uuid::new_v4().to_string(),
                            act.name(),
                            err_msg,
                            0,
                        );
                        collected[idx] = Some((act, outcome, None));
                    }
                }
            }
        }

        // Commit all action outcomes in strict original batch order
        for (idx, slot) in collected.into_iter().enumerate() {
            let (act, outcome, task_state_opt) = match slot {
                Some(data) => data,
                None => {
                    let act = batch_actions[idx].clone();
                    let outcome = ActionResult::failed(
                        uuid::Uuid::new_v4().to_string(),
                        act.name(),
                        format!("Task at batch index {idx} missing result"),
                        0,
                    );
                    (act, outcome, None)
                }
            };

            let target_goal = act.associated_goal_id().map(|s| s.to_string());
            if let Some(task_state) = task_state_opt {
                // Durably merge any knowledge items produced by the concurrent task scoped strictly to the target goal
                for (id, item) in task_state.knowledge_items {
                    if !self.controller.state.knowledge_items.contains_key(&id) {
                        let belongs_to_goal = match &target_goal {
                            Some(gid) => item.related_goals.contains(gid),
                            None => true,
                        };
                        if belongs_to_goal {
                            self.controller.state.add_knowledge_item(item);
                        }
                    }
                }
                // Sync goal failure if worker task recorded a failed status for its target goal
                if let Some(ref gid) = target_goal
                    && let Some(task_goal) = task_state.goals.get_goal(gid)
                    && task_goal.status == crate::cognition::GoalStatus::Failed
                    && let Some(auth_goal) = self.controller.state.goals.get_goal_mut(gid)
                {
                    auth_goal.status = crate::cognition::GoalStatus::Failed;
                }
                // Merge any new contradictions detected during the task
                for contradiction in task_state.contradictions {
                    if !self
                        .controller
                        .state
                        .contradictions
                        .iter()
                        .any(|c| c.id == contradiction.id)
                    {
                        self.controller.state.contradictions.push(contradiction);
                    }
                }
            } else {
                // Task failed without valid state; mark goal failed in authoritative state
                if let Some(ref gid) = target_goal
                    && let Some(auth_goal) = self.controller.state.goals.get_goal_mut(gid)
                {
                    auth_goal.status = crate::cognition::GoalStatus::Failed;
                }
            }

            // Commit outcome authoritatively
            let continue_exec = self.controller.step_commit(&act, outcome)?;
            if !continue_exec || self.controller.state.is_stopped() {
                break;
            }
        }

        self.persist_state_and_events().await?;
        Ok(!self.controller.state.is_stopped())
    }

    /// Run the runtime until the investigation completes, stops, or hits limits.
    pub async fn run(&mut self) -> Result<(), String> {
        while !self.controller.state.is_stopped() {
            let continue_exec = self.step().await?;
            if !continue_exec {
                break;
            }
        }
        self.persist_state_and_events().await?;
        Ok(())
    }

    /// Atomically persist cognitive state snapshot and any new cognitive events to SQLite.
    pub async fn persist_state_and_events(&mut self) -> Result<(), String> {
        let pending_events = self.controller.events.clone();
        self.store
            .persist_cognitive_snapshot_and_events(
                self.investigation_id.clone(),
                &self.controller.state,
                pending_events,
            )
            .await
            .map_err(|e| e.to_string())?;

        // Durably committed to SQLite: safely clear pending events from memory
        self.controller.events.clear();
        Ok(())
    }

    pub fn selected_action(&self) -> Option<&ExecutiveAction> {
        self.controller.state.executive.selected_action.as_ref()
    }

    pub fn state(&self) -> &CognitiveState {
        &self.controller.state
    }

    pub fn state_mut(&mut self) -> &mut CognitiveState {
        &mut self.controller.state
    }
}
