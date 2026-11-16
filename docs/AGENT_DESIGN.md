# Specialist Agent Architecture & Scheduling Design

## 1. Domain Specialist Roles

NeuroGenesis AI orchestrates five distinct scientific domain specialists. Each specialist is governed by a strict role prompt and an explicit allowlist of permitted computational tools:

| Specialist Role | Domain Responsibilities | Allowed Scientific Tools |
| :--- | :--- | :--- |
| **Mathematics** | Deterministic calculations, arithmetic, statistical analysis, curve fitting, polynomial evaluation | `arithmetic`, `statistics`, `power_law`, `polynomial`, `symbolic`, `linear_algebra`, `ode_solve` |
| **Physics** | Classical mechanics, Newtonian kinematics, gravitational ballistics, conservation laws | `projectile`, `arithmetic`, `power_law`, `ode_solve`, `statistics` |
| **Chemistry** | Stoichiometry, physicochemical properties, reaction mechanics | `arithmetic`, `statistics`, `symbolic` |
| **Biology** | Cellular kinetics, genetics, sequence analysis | `statistics`, `power_law`, `linear_algebra` |
| **Medicinal Science** | Pharmacokinetics, dosing metrics, therapeutic indicators | `arithmetic`, `statistics`, `power_law` |

Specialists are strictly prohibited from generating executable shell scripts or arbitrary code. All calculations must be dispatched to allowlisted tools.

## 2. Bounded Asynchronous Scheduling Architecture

Specialist execution is governed by a bounded, asynchronous, dependency-aware scheduler implemented in Rust (`src/orchestrator.rs`).

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
     │ (Indep. Model Request)  │   │ (Receives Task A tools) │
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

### 2.1 Task Identity by ID (No Role Deduplication)
Subproblems are defined strictly by their unique task `id` (e.g. `sub-1`, `sub-2`), NOT by their specialist role name. A research plan containing multiple distinct mathematics subproblems (e.g. one for addition, one for multiplication) executes all subproblems without deduplication.

### 2.2 Dependency Context Propagation
Downstream tasks that declare dependencies via `depends_on` receive the completed tool executions and specialist reports of their direct and transitive dependencies in their execution context (`tool_results` and `dependency_reports`). Independent subproblems execute concurrently without waiting.

### 2.3 Dependency Failure Surfacing & Failure Isolation
- **Dependency Failure Surfacing:** If a prerequisite task fails, downstream dependent subproblems are immediately marked as `AgentExecutionStatus::Failed` with an explicit reason (e.g. `Dependency 'step-1' failed, preventing execution.`) without launching broken downstream tasks.
- **Sibling Failure Isolation:** Failures in one task branch do not abort independent sibling branches or overall synthesis.

### 2.4 Concurrency Configuration
Specialist concurrency is configured via the `SPECIALIST_CONCURRENCY` environment variable:
- Clamped strictly between `1` and `5` (default: `3`).
- **Serial Execution Mode:** When `SPECIALIST_CONCURRENCY=1`, the scheduler enforces sequential dispatch while preserving full dependency ordering, context propagation, and failure isolation.
- **Concurrent Mode:** When `SPECIALIST_CONCURRENCY > 1`, tasks whose dependencies are met execute concurrently up to the configured limit using Tokio tasks in a `tokio::task::JoinSet`.

## 3. Cognitive State & Executive Integration

In Milestone 1.3, specialist subproblems are mapped to typed internal goals within `CognitiveState` (`src/cognition/state.rs`). The internal `ExecutiveController` tracks goal statuses (`Pending`, `Active`, `Completed`, `Blocked`, `Failed`, `Cancelled`), working memory activation, and selective attention priorities independently of external model providers.
