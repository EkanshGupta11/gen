# NeuroGenesis Cognitive Runtime Architecture (Milestone 1.3.1)

## 1. Executive Summary & Objective

In Milestone 1.3.1, the NeuroGenesis cognitive architecture transitioned from independently tested modular components into an active, authoritative cognitive runtime governing production scientific investigations.

The architectural mandate is unequivocal:
- **No LLM wrapper**: Reasoning, goal management, memory policies, action selection, and verification are owned by deterministic Rust systems code.
- **Epistemic integrity**: Epistemic statuses (`Verified`, `Calculated`, `Contradicted`, `Hypothesis`, `Uncertain`, `EvidenceSupported`) cannot be self-assigned, hallucinated, or bypassed by external models.
- **Evidence-based contradiction arbitration**: Contradictions require independent verification artifacts and structured resolution outcomes.
- **Durable cognitive state**: Active goals, working memory, attention records, loop history, and execution budgets persist across application lifecycles in SQLite.

---

## 2. Core Architectural Components

The runtime boundary enforces clean separation of concerns across four primary structs:

```
┌────────────────────────────────────────────────────────┐
│                   Orchestrator                         │
│   (Translates research questions, seeds goals & agenda)│
└──────────────────────────┬─────────────────────────────┘
                           │
                           ▼
┌────────────────────────────────────────────────────────┐
│                 CognitiveRuntime                       │
│   (Async event loop, persistence synchronization,      │
│    cycle orchestration)                                │
└─────────────┬────────────────────────────┬─────────────┘
              │                            │
              ▼                            ▼
┌────────────────────────────┐ ┌─────────────────────────┐
│    ExecutiveController     │ │   AsyncActionExecutor   │
│ (Pure cognitive control,   │ │ (Async tool execution,  │
│  observe, goal evaluation, │ │  Python worker invoke,  │
│  attention focus, policy)  │ │  independent verifier)  │
└────────────────────────────┘ └─────────────────────────┘
```

### 2.1 `CognitiveRuntime` (`src/cognition/runtime.rs`)
- **Role**: Coordinates the discrete cycle lifecycle asynchronously.
- **Cycle Flow**:
  1. Calls `ExecutiveController::step_prepare()` to evaluate goals, focus attention, select the next action, and validate preconditions.
  2. Dispatches the action to `AsyncActionExecutor::execute()` for real external interactions (worker invocation, model synthesis, verification).
  3. Commits the resulting `ActionResult` into `ExecutiveController::step_commit()`, updating working memory, agenda items, and metacognitive monitoring.
  4. Flushes state snapshots and append-only cognitive events into SQLite via `Store`.

### 2.2 `ExecutiveController` (`src/cognition/executive.rs`)
- **Role**: Deterministic state machine governing internal cognitive processes.
- **Phases**:
  `Observe` → `EvaluateGoals` → `UpdateState` → `AttentionPrioritisation` → `SelectAction` → `ValidatePreconditions` → `ExecuteAction` → `EvaluateOutcome` → `UpdateWorkingMemory` → `MetacognitiveEvaluation` → `Stopped`.
- **Invariants**:
  - Independent of network I/O; can run synchronously in memory for deterministic unit testing.
  - Enforces hard execution bounds (`max_cycles`, `max_actions`, `timeout`).
  - Restores exact historical budget and loop detector state on resumption.

### 2.3 `AsyncActionExecutor` (`src/cognition/executor.rs`)
- **Role**: Performs side-effecting operations while upholding epistemic guardrails.
- **Capabilities**:
  - `ExecuteScientificTool`: Validates tool against `ALLOWED_SCIENTIFIC_TOOLS`, executes via `worker::invoke` in CPython 3.13, runs deterministic Rust verifier (`verification::verify_tool`), and stores results as typed `ToolExecution` and `KnowledgeItem`.
  - `RequestSpecialistAnalysis`: Invokes model provider with bounded retry and token accounting, executes requested specialist tool calls, bounds tool execution per role (max 3), and enforces epistemic downgrade on unverified claims.
  - `VerifyCalculation`: Recovers calculation knowledge items by ID or tool name, executes independent Rust checkers, and updates epistemic status to `Verified` or `Contradicted`.
  - `ResolveContradiction`: Arbitrates conflicting items based on independently verified evidence, rejecting unverified items.
  - `SynthesizeFindings`: Summarizes multi-specialist outputs, explicitly barring models from self-certifying findings as `Verified`.

---

## 3. Epistemic Status & Integrity Rules

Epistemic classification reflects empirical justification:

| Epistemic Status | Source / Requirement | Can Model Self-Assign? |
|---|---|:---:|
| `Verified` | Must pass independent deterministic Rust checker within tolerance ($10^{-4}$) | **NO** |
| `Calculated` | Deterministic worker execution without formal independent verification | **NO** |
| `Contradicted` | Divergence between worker calculation and independent Rust checker | **NO** |
| `EvidenceSupported` | Attached to verified empirical data or primary scholarly literature | **NO** |
| `Hypothesis` | Candidate proposal or claim requiring empirical testing | YES |
| `Uncertain` | Execution failure, boundary violation, or unverified mock output | YES |

---

## 4. Evidence-Based Contradiction Resolution

Contradictions are tracked as first-class cognitive records (`ContradictionRecord`). Resolution requires structured evidence:

```rust
pub struct ContradictionResolution {
    pub contradiction_id: String,
    pub conflicting_items: Vec<String>,
    pub proposed_resolution: String,
    pub supporting_evidence_refs: Vec<String>,
    pub verification_method: String,
    pub outcome: ResolutionOutcome,
    pub timestamp: String,
}
```

- When `ResolveContradiction` executes, it verifies that at least one conflicting item has been verified by an independent checker.
- Contradicted items are marked rejected, verified items are retained, and the record transitions to `ResolutionOutcome::Resolved`.
- If no verified evidence is present, the contradiction remains unresolved with status `Uncertain`, blocking premature closure.

---

## 5. Persistence & Recovery Semantics

Persistence is managed atomically through SQLite:
1. **Snapshots**: Every completed action commits the full `CognitiveState` JSON into `investigations.cognitive_state_json`.
2. **Event Sinks**: All discrete cycle events (`CycleStarted`, `ActionSelected`, `ActionExecuted`, `AttentionFocused`, `MetacognitiveEvaluation`, etc.) are appended to the `cognitive_events` table.
3. **Resumption**:
   - `CognitiveRuntime::recover(investigation_id, config, executor, store)` loads the serialized state.
   - `ExecutiveController::from_existing_state()` restores:
     - `total_actions_executed`: Preserves the remaining budget across crashes.
     - `loop_detection_history`: Preserves action frequency counts to prevent infinite loop exploits across restarts.
     - Completed and active goal structures and agenda items.
   - Execution resumes seamlessly at the next eligible agenda item without re-executing completed prerequisites.

---

## 6. Metacognitive Monitoring & Loop Prevention

The executive controller runs metacognitive monitoring on every discrete cycle:
- **Consecutive Failure Detection**: If 3 consecutive actions fail, execution aborts with `StopReason::MetacognitiveAbort`.
- **Sliding-Window Loop Detection**: Action fingerprints (`action_name:target_goal`) are tracked across a sliding window of size 6. If repeated identical actions are attempted, execution halts immediately with `StopReason::MetacognitiveAbort`.
- **Failure Isolation**: Subproblem failures cleanly mark downstream dependencies as `Blocked` while allowing the overall investigation to complete via failure-tolerant `SynthesizeFindings`.

---

## 7. Milestone 1.3.2 Runtime Reliability & Epistemic Closures

Milestone 1.3.2 addresses six critical operational findings from independent architectural reviews:

### 7.1 Runtime Error Propagation & Honest Outcome Mapping
- `src/orchestrator.rs` inspects the outcome of `runtime.run().await`. Runtime aborts and failures propagate immediately, marking the record as `InvestigationStatus::Failed`.
- Introduced `InvestigationStatus::Interrupted` into `src/contracts.rs`.
- `map_stop_reason_to_investigation_outcome` explicitly translates every `StopReason`:
  - `ObjectiveAchieved` / `AllGoalsCompleted` $\to$ `InvestigationStatus::Completed`
  - `BudgetExhausted` / `CycleLimitReached` / `TimeoutExceeded` / `UserCancelled` $\to$ `InvestigationStatus::Interrupted`
  - `DeadlockNoExecutableActions` / `MetacognitiveAbort` $\to$ `InvestigationStatus::Failed`
  - Incomplete state stops $\to$ `InvestigationStatus::Interrupted`
- Resource exhaustion, timeout, or deadlock can never masquerade as successful completion.

### 7.2 Atomic SQLite Persistence & Pending Event Retention
- `Store::persist_cognitive_snapshot_and_events` executes the snapshot write and event log insertions inside a single atomic SQLite transaction (`tx`).
- If an event insertion or disk operation fails, both snapshot and events roll back completely with zero database corruption.
- Pending events in `CognitiveRuntime` are strictly preserved in memory on persistence error and are cleared only after a confirmed SQLite commit.

### 7.3 Dynamic Specialist Provenance
- Removed hardcoded `Specialist::Mathematics` default for tool executions.
- `resolve_specialist_for_tool_execution` resolves the initiating scientific specialist dynamically from goal completion criteria, goal descriptions, parent goals, linked specialist reports, and tool allowlists.
- Provenance attributes the real specialist domain (e.g. `Specialist::Physics` for projectile ballistics).

### 7.4 Bounded Concurrency Scheduling & State Merging
- Bounded parallel execution is preserved via `max_concurrency` while upholding executive control invariants:
  - `ExecutiveController::get_concurrent_ready_actions` schedules ready actions for distinct active subproblems with zero mutual dependencies.
  - Subproblems execute concurrently using Tokio `JoinSet`.
  - Mutated state (knowledge items, working memory items, and contradictions) is collected and merged sequentially into the authoritative controller state, ensuring that goal completions and dependency transitions remain fully consistent and deterministic.

### 7.5 Rigorous Contradiction Arbitration & Comparability Checks
- A verified calculation is not automatically treated as proof that an entire contradiction is resolved.
- `check_contradiction_comparability` verifies:
  1. Tool identity: Matching tools/scientific procedures.
  2. Condition identity: Parameter closeness within numerical tolerance $10^{-4}$.
  3. Unit compatibility: Incompatible unit strings (e.g. meters vs feet) prevent automated resolution.
  4. Assumption compatibility: Conflicting physical assumptions (e.g. ideal vacuum vs air drag) prevent automated resolution.
- Ambiguous contradictions or cases with multiple conflicting verified items route to `ResolutionOutcome::RequiresHumanReview` with `resolved = false` and `ActionExecutionStatus::Uncertain`.

---

## 8. Milestone 1.3.2.1 Executive Control Hardening & Live Model Validation

Milestone 1.3.2.1 eliminates executive bypass vulnerabilities and hardens verification and persistence invariants:

### 8.1 Authoritative Batch Preparation Lifecycle (`step_prepare_batch`)
- All actions—including concurrent batches—route strictly through `ExecutiveController::step_prepare_batch(max_batch)`.
- Enforces cycle limits, timeouts, and action budget bounds **before** any batch action is dispatched.
- Action budget reservation: parallel batch capacity is strictly clamped to `max_batch.min(max_concurrency).min(remaining_action_budget)`.
- Loop detection covers **every action** in the batch; repeated actions trigger immediate `MetacognitiveAbort`.
- Precondition validation runs for every action in the batch before task spawning.
- Bidirectional dependency validation prevents goals with mutual or sequential dependencies from entering the same batch.
- Worker task mutation isolation: tasks execute on state clones, and only knowledge items strictly scoped to the task's target goal are merged into authoritative state upon completion.
- Sequential authoritative state commits via `step_commit(&act, outcome)` preserve deterministic action accounting, loop history, and event streams.

### 8.2 Fail-Closed Contradiction Comparability (`check_contradiction_comparability`)
- Missing metadata fails closed: missing tools, missing arguments, missing units, missing explicit assumptions, or missing output result fields return `Err`.
- Conflicting physical assumptions (vacuum vs air drag), model assumptions (relativistic vs classical), thermodynamic assumptions (isothermal vs adiabatic), or fluid assumptions (laminar vs turbulent) fail comparability.
- Incomparable or ambiguous evidence routes to `ResolutionOutcome::RequiresHumanReview` with `resolved = false`.

### 8.3 Cancelled-Goal Completion Semantics & Scoped Criteria
- Cancelled goals never count as completed in all-goals-complete logic.
- Partial or full cancellation maps to `StopReason::UserCancelled` $\to$ `InvestigationStatus::Interrupted`.
- Goal completion criteria are strictly scoped to the target goal: tools executed for Goal A cannot satisfy completion criteria for Goal B.

### 8.4 Failure-Persistence Error Surfacing
- Every store failure when persisting error records is surfaced alongside the primary error, eliminating swallowed persistence errors.

### 8.5 Live Gemini Provider Integration
- Validated Google Gemini OpenAI-compatible endpoint compatibility (`https://generativelanguage.googleapis.com/v1beta/openai/`).
- Documented model catalog availability (`models/gemini-3.8-flash`, `models/gemini-flash-latest`) and upstream cloud demand responses (HTTP 503 / timeout) handled safely by exponential backoff and honest error reporting without leaking credentials.
- Offline mock mode remains the fully operational, deterministic default.


