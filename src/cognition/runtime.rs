use crate::{
    cognition::{
        actions::ExecutiveAction,
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

        match self.controller.step_prepare()? {
            Some(action) => {
                self.controller.state.executive.current_phase = ExecutivePhase::ExecuteAction;
                let outcome = self
                    .executor
                    .execute(&action, &mut self.controller.state)
                    .await;
                self.controller.step_commit(&action, outcome)?;
                self.persist_state_and_events().await?;
                Ok(!self.controller.state.is_stopped())
            }
            None => {
                self.persist_state_and_events().await?;
                Ok(!self.controller.state.is_stopped())
            }
        }
    }

    /// Run the runtime until the investigation completes, stops, or hits limits.
    pub async fn run(&mut self) -> Result<&CognitiveState, String> {
        while !self.controller.state.is_stopped() {
            let continue_exec = self.step().await?;
            if !continue_exec {
                break;
            }
        }
        self.persist_state_and_events().await?;
        Ok(&self.controller.state)
    }

    /// Atomically persist cognitive state snapshot and any new cognitive events to SQLite.
    pub async fn persist_state_and_events(&mut self) -> Result<(), String> {
        let events = std::mem::take(&mut self.controller.events);
        if !events.is_empty() {
            let _ = self.store.append_cognitive_events(events).await;
        }
        self.store
            .save_cognitive_state(self.investigation_id.clone(), &self.controller.state)
            .await
            .map_err(|e| e.to_string())?;
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
