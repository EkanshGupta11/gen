# Specialist Agent Architecture & Scheduling Design

## 1. Domain Specialist Roles

NeuroGenesis AI orchestrates five distinct scientific domain specialists. Each specialist is governed by a strict role prompt and an explicit allowlist of permitted computational tools:

| Specialist Role | Domain Responsibilities | Allowed Scientific Tools |
| :--- | :--- | :--- |
| **Mathematics** | Deterministic calculations, arithmetic, statistical analysis, curve fitting, polynomial evaluation | `arithmetic`, `statistics`, `power_law`, `polynomial` |
| **Physics** | Classical mechanics, Newtonian kinematics, gravitational ballistics, conservation laws | `projectile` |
| **Chemistry** | Stoichiometry, physicochemical properties, reaction mechanics | allowlisted chemistry tools |
| **Biology** | Cellular kinetics, genetics, sequence analysis | allowlisted biology tools |
| **Medicinal Science** | Pharmacokinetics, dosing metrics, therapeutic indicators | allowlisted clinical/medicinal tools |

Specialists are strictly prohibited from generating executable shell scripts or arbitrary code. All calculations must be dispatched to allowlisted tools.

## 2. Bounded Asynchronous Scheduling Architecture

In Milestone 1.2.1, specialist execution has been upgraded from serial loops to a bounded, asynchronous, dependency-aware scheduler implemented in Rust (`src/orchestrator.rs`).

```
                    ┌─────────────────────────┐
                    │  Planning Decomposition │
                    └────────────┬────────────┘
                                 │
                     [Subproblems with DAG deps]
                                 │
                                 ▼
             ┌───────────────────────────────────────┐
             │   Asynchronous Specialist Scheduler   │
             │     (Tokio JoinSet, max concurrency)  │
             └───────┬───────────────────────┬───────┘
                     │                       │
     ┌───────────────▼─────────┐   ┌─────────▼───────────────┐
     │ Specialist Task A       │   │ Specialist Task B       │
     │ (Indep. Model Request)  │   │ (Indep. Model Request)  │
     └───────────────┬─────────┘   └─────────┬───────────────┘
                     │                       │
            [Worker Tool Dispatch]  [Worker Tool Dispatch]
                     │                       │
          [Rust Indep. Verification] [Rust Indep. Verification]
                     │                       │
                     └───────────┬───────────┘
                                 │
                   (Resolves downstream deps)
                                 │
                                 ▼
                    ┌─────────────────────────┐
                    │    Synthesis Stage      │
                    │ (Epistemic Enforcement) │
                    └─────────────────────────┘
```

### 2.1 Concurrency Configuration
Specialist concurrency is configured via the `SPECIALIST_CONCURRENCY` environment variable:
- Clamped strictly between `1` and `5` (default: `3`).
- **Serial Execution Mode:** When `SPECIALIST_CONCURRENCY=1`, the orchestrator falls back to sequential specialist dispatch, guaranteeing deterministic ordering for debugging and constrained environments.
- **Concurrent Mode:** When `SPECIALIST_CONCURRENCY > 1`, tasks whose dependencies are met execute concurrently up to the configured limit using Tokio tasks in a `tokio::task::JoinSet`.

### 2.2 Dependency DAG Resolution
1. The planner decomposes the question into subproblems, each with an identifier (`id`) and a list of required prior subproblems (`depends_on`).
2. The scheduler dynamically evaluates ready subproblems where all `depends_on` IDs have completed.
3. As each task finishes, its output, tool executions, and verification results are persisted to SQLite, its ID is marked completed, and newly unblocked dependent subproblems are scheduled into the `JoinSet`.
4. Unsatisfied or cyclic dependencies are cleanly drained and recorded with `AgentExecutionStatus::Failed` to ensure deadlock-free termination.

### 2.3 Failure Isolation
A failure in one specialist (e.g., model timeout, malformed JSON response, or worker crash) is strictly isolated:
- The failed specialist generates an `AgentReport` with `status: Failed` and records the error description.
- Independent concurrent specialists continue executing without disruption.
- The synthesis phase receives the failed report and explicitly incorporates the failure into its uncertainties summary rather than aborting the investigation.

## 3. Independent Model Execution & Budgeting

When hosted inference is enabled:
- **Distinct Requests:** Distinct specialist tasks issue independent HTTP requests with role-specific system prompts.
- **Request Budgeting (`RequestBudget`):**
  - Logical call limits: Enforces a maximum number of model requests per investigation.
  - HTTP attempt limits: Enforces bounded retries (maximum 3 attempts).
  - Token budget: Enforces cumulative token limits ($prompt + completion \le max\_tokens$).
- **Offline Mock Provider:** When `MODEL_PROVIDER=mock`, the orchestrator uses a deterministic offline mock provider. Mock reports are explicitly labeled with `status: Mock`, and no hosted inference is claimed.

## 4. Epistemic Constraints on Agents

1. **No Self-Verification:** Models cannot assign `verified` or `evidence_supported` to their own claims.
2. **Tool Verification Requirement:** Only tools verified by native Rust checkers receive `EpistemicStatus::Verified`.
3. **Synthesis Epistemic Transfer:**
   - Synthesis claims referencing verified tools inherit `EpistemicStatus::Verified`.
   - Claims referencing unverified tools inherit `EpistemicStatus::Calculated`.
   - Claims without supporting tools or literature sources default strictly to `EpistemicStatus::Hypothesis`.
