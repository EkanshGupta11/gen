# NeuroGenesis AI — Master Architectural Context

## Project Identity & Canonical Boundaries

- **Repository:** `https://github.com/EkanshGupta11/gen.git` (Public repository)
- **Local Canonical Workspace:** `C:\Users\Ekansh\Documents\Codex\NeuroGenesis`
- **Active Development Branch:** `m1.3.1-cognitive-runtime`
- **Core Technology Stack:**
  - Systems Core: Rust 2024 edition (`axum`, `tokio`, `rusqlite`, `serde`, `reqwest`)
  - Scientific Runtime: CPython 3.13.15 managed by Astral `uv` (`numpy`, `scipy`, `sympy`, `pandas`, `matplotlib`)
  - Storage: SQLite 3 with WAL mode, foreign keys enabled, user_version migrations
  - Frontend: Light-first responsive research workspace (HTML5, CSS, vanilla ES2022)

---

## Non-Negotiable System Principles

1. **Autonomous Cognitive Control:** NeuroGenesis AI is not a wrapper around an LLM. An external model may optionally assist with ambiguous language interpretation or specialist hypothesis generation, but the cognitive engine owns all state transitions, memory replacement, goal evaluation, attention prioritization, and action dispatch.
2. **Explicit Epistemic Distinctions:**
   - `verified`: Independently checked by deterministic Rust code or formal proof.
   - `calculated`: Produced by an allowlisted deterministic computation tool (e.g. NumPy/SciPy).
   - `evidence_supported`: Backed by primary scholarly literature with verified provenance.
   - `hypothesis`: Plausible model-proposed claim lacking independent verification.
   - `uncertain`: Stated ambiguity, boundary condition, or insufficient data.
   - `contradicted`: Disproven by empirical contradiction or mathematical inconsistency.
3. **Model Prohibitions:** Models are strictly barred from self-assigning `verified` status to their own claims or outputs.
4. **No Notion:** Notion is permanently excluded.
5. **Secret Safety:** Never commit `.env`, credentials, local database files, virtual environments, or caches to Git.

---

## Module Structure

```
src/
├── api.rs            # Axum HTTP API endpoints (/api/investigate, /api/history, /api/tool, etc.)
├── contracts.rs      # Strongly typed domain, specialist, and verification contracts
├── main.rs           # Application initialization, server configuration, and signal handling
├── orchestrator.rs   # Multi-specialist scheduling with dynamic DAG dependency propagation
├── provider.rs       # Hosted and deterministic mock model providers with token budgeting
├── store.rs          # Rust-owned SQLite persistence with versioned migrations (schema v2)
├── verification.rs   # Deterministic closed-form Rust checkers (projectile, arithmetic, statistics, power-law)
├── worker.rs         # Subprocess invocation bridge to CPython 3.13 scientific runtime
└── cognition/        # Milestone 1.3 & 1.3.1 Cognitive Runtime & Control Foundation
    ├── mod.rs             # Subsystem re-exports and unit tests
    ├── actions.rs         # Typed executive actions, subgoal definitions, and stop reasons
    ├── attention.rs       # Selective attention engine with deterministic scoring
    ├── events.rs          # Auditable append-only cognitive event contracts
    ├── executive.rs       # 10-phase executive control loop and loop prevention
    ├── executor.rs        # Asynchronous external action executor with Python worker & verification
    ├── goals.rs           # Hierarchical goals, dependency graphs, and completion criteria
    ├── policy.rs          # Action precondition checks and rolling loop detector
    ├── runtime.rs         # Authoritative async cognitive runtime with event/state persistence
    ├── state.rs           # Strongly typed serializable CognitiveState
    └── working_memory.rs  # Bounded working memory with retention scoring and protection
engine/
├── app.py            # Sandboxed Python scientific tools (arithmetic, projectile, power_law, etc.)
└── test_app.py       # Python unit tests for tool correctness
web/
└── index.html        # Clean, light-first responsive research interface displaying cognitive state
```

---

## Operating Guidelines for Engineers & Agents

- **Offline Verifiability:** All cognitive mechanisms and verification suites must run and pass without network connectivity or API credentials.
- **Dependency Flow:** When subproblems declare `depends_on`, the orchestrator propagates completed tool executions and specialist reports to all downstream tasks.
- **Task Identity:** Task identity is defined by task IDs, not specialist roles. Multiple subproblems sharing a specialist role must never be deduplicated.
- **Database Ownership:** Rust (`src/store.rs`) exclusively owns SQLite database reads, writes, and migrations. Python is strictly a calculation worker.
