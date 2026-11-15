# NeuroGenesis AI Architecture

## System Boundaries & Component Ownership

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Rust Core (Axum / Tokio)                        │
│                                                                        │
│  ┌───────────────────────┐   ┌──────────────────────────────────────┐  │
│  │   HTTP & Static API   │   │      Investigation Orchestrator      │  │
│  │    (Contracts 1.0)    │   │  - Planning & Synthesis Routing      │  │
│  └───────────┬───────────┘   │  - Bounded Asynchronous Scheduler    │  │
│              │               │    (Tokio JoinSet, Concurrency 1..5) │  │
│              │               └──────────────────┬───────────────────┘  │
│              │                                  │                      │
│              │       ┌──────────────────────────┴───────────────┐      │
│              │       │       Independent Verification Engine    │      │
│              │       │  - Newtonian Ballistics Derivation       │      │
│              │       │  - Exact Arithmetic Validation           │      │
│              │       │  - Descriptive Statistics Invariants     │      │
│              │       │  - Power-Law Least-Squares Normal Eqns   │      │
│              │       └──────────────────────────┬───────────────┘      │
│              │                                  │                      │
│  ┌───────────▼───────────┐                      │                      │
│  │   SQLite Store        │◄─────────────────────┘                      │
│  │   (rusqlite, backup)  │                                             │
│  └───────────────────────┘                                             │
└──────────────┬──────────────────────────────────┬──────────────────────┘
               │                                  │
     (Versioned JSON Stdin/Stdout)         (HTTP / JSON Mode)
               │                                  │
               ▼                                  ▼
┌───────────────────────────────┐  ┌─────────────────────────────────────┐
│  Python Scientific Worker     │  │  Model Provider                     │
│  (CPython 3.13 Virtualenv)    │  │  - OpenAI-compatible hosted API     │
│  - NumPy, SciPy, SymPy        │  │  - Offline deterministic mock       │
│  - Allowlisted Tools Only     │  │  - Bounded retries & token budget   │
└───────────────────────────────┘  └─────────────────────────────────────┘
```

### 1. Rust Application Core (`Axum`, `Tokio`, `rusqlite`)
- **Orchestration & Workflow:** Owns planning, specialist dispatch, tool execution coordination, synthesis, and final status classification.
- **Independent Verification:** Natively executes closed-form analytical verifications in Rust, completely independent of the Python worker.
- **Bounded Asynchronous Scheduler:** Manages specialist concurrency via `tokio::task::JoinSet` with configurable limits (`SPECIALIST_CONCURRENCY`, 1..=5, default 3). Resolves subproblem dependency DAGs dynamically and isolates task failures.
- **Persistence:** Direct SQLite integration with additive schema migrations, automatic pre-migration snapshots, and cross-process database recovery.

### 2. Python Scientific Computation Worker (`engine/app.py`)
- **Execution Model:** Subprocess executed within a dedicated CPython 3.13 virtual environment (`.venv-cpython313`). Communicates over stdin/stdout using versioned JSON contract `1.0`.
- **Role:** Pure mathematical and scientific calculations.
- **Security & Isolation:** Has no network access, model credentials, file system write permissions, or database connections. Cannot execute arbitrary code or shell commands.

### 3. Model Provider Transport (`src/provider.rs`)
- **Interface:** Abstract `ModelProvider` trait supporting `openai_compatible` and `mock`.
- **Reliability:** Enforces structured JSON output matching requested schemas, bounded retries on 500/503 server errors with exponential backoff, rate limit handling, and token budgets.
- **Mock Mode:** Explicitly labels outputs as mock and performs no hosted scientific analysis.

---

## The Investigation Lifecycle

1. **Planning Stage:**
   - Rust creates an initial `Investigation` record in SQLite with status `Planning`.
   - The planner decomposes the question into up to 5 specialist subproblems with explicit `id`, `specialist`, `task`, `depends_on`, and `required_tools` fields.
2. **Specialist Execution Stage:**
   - Subproblems are scheduled based on `SPECIALIST_CONCURRENCY`. If concurrency $\le 1$, subproblems run sequentially; otherwise, independent ready subproblems run in parallel.
   - For each specialist, a distinct prompt is submitted to the model provider with role instructions.
   - Specialist tool calls are validated against the role's allowlist (up to 3 tool calls per specialist).
   - Tool arguments are dispatched to the Python worker.
   - Python worker results are immediately routed into the Rust independent verification engine.
   - Verification results are recorded with exact errors, tolerances, and status (`verified`, `calculated`, `contradicted`, or `uncertain`).
   - SQLite is updated after each specialist completes to guarantee crash resilience.
3. **Synthesis Stage:**
   - The synthesis stage receives specialist reports, executed tool outputs, and verification results.
   - **Epistemic Classification Rules:**
     - A model cannot self-assign `verified` or `evidence_supported`.
     - Claims referencing independently verified tools receive `verified`.
     - Claims referencing unverified calculations receive `calculated`.
     - Claims lacking source backing or verified calculations remain `hypothesis`.
   - Final report is persisted to SQLite with status `Completed`.

---

## Concurrency and Scheduling Rules

1. **Bound Limits:** `SPECIALIST_CONCURRENCY` is clamped to $1 \le C \le 5$.
2. **Dependency Resolution:** A subproblem is only scheduled when all dependencies in its `depends_on` list have completed.
3. **Deadlock Prevention:** Unsatisfied or cyclic dependencies are drained and recorded with `status: Failed`.
4. **Failure Isolation:** A model timeout or tool failure in one specialist does not abort concurrent tasks or prevent synthesis.

---

## Current Status and Limitations

- **Hosted Model Credentials:** Safely verified; none configured in `.env`. Offline mock mode is fully operational.
- **Scholarly Literature Retrieval:** OpenAlex, Crossref, and PubMed integrations will be added in subsequent milestones to support `evidence_supported` claims.
- **Formal Verification:** Lean 4 integration is designed as an extension point for formal mathematical proofs.
- **Long-Term Memory:** Zep conversational memory is planned for future milestones.
