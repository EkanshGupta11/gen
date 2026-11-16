# NeuroGenesis AI — Development Roadmap

This document outlines completed milestones, current progress, and future development horizons for NeuroGenesis AI.

---

## Completed Milestones

### Milestone 1.1: Local Baseline & Scientific Runtime
- Established canonical local workspace in Windows environment.
- Configured CPython 3.13.15 runtime with Astral `uv` package management.
- Pre-installed scientific packages: NumPy 2.5.5, SciPy 1.18.1, SymPy 1.14.0, pandas 3.1.1, Matplotlib 3.11.2.
- Implemented five scientific specialist roles and offline mock mode.
- Built light-first browser interface.

### Milestone 1.2: Independent Deterministic Verification
- Created typed `VerificationResult` contract.
- Implemented independent closed-form Rust verification checkers for:
  - Newtonian projectile kinematics ($R = v^2 \sin(2\theta)/g$).
  - Exact arithmetic operations.
  - Bessel-corrected sample statistics.
  - Power-law least-squares normal equation consistency.
- Enforced strict epistemic statuses: `verified`, `calculated`, `evidence_supported`, `hypothesis`, `uncertain`, `contradicted`.
- Established non-negotiable rule: external models cannot self-assign `verified` status.

### Milestone 1.2.1: Specialist Scheduling & Reliability Validation
- Upgraded specialist execution to bounded asynchronous scheduler using Tokio `JoinSet`.
- Implemented failure isolation: individual specialist model or tool errors do not terminate sibling tasks or synthesis.
- Configured per-role model request routing.
- Validated provider token budgeting and exponential backoff retry policies.

### Milestone 1.3: Cognitive Control Foundation (Current Milestone)
- **Phase A — Orchestration Bug Fixes:**
  - Removed specialist role deduplication; subproblems are identified strictly by unique task IDs (`sub.id`). Multiple distinct tasks under the same specialist role (e.g. multiple mathematics subproblems) are preserved and executed.
  - Corrected dependency context propagation: downstream tasks receive completed tool outputs and reports from all direct and transitive dependencies.
  - Handled dependency failure surfacing: downstream tasks depending on failed tasks are surfaced as failed without executing.
  - Unified serial (`max_concurrency: 1`) and concurrent (`max_concurrency: 2..=5`) dependency scheduling.
- **Phase B — Cognitive State Model:**
  - Created strongly typed, versioned `CognitiveState` (`src/cognition/state.rs`) representing investigation context, hierarchical goals, hypotheses, knowledge items, working memory, executive controller state, and contradiction tracking.
- **Phase C — Bounded Working Memory Engine:**
  - Implemented `WorkingMemory` (`src/cognition/working_memory.rs`) with configurable capacity (default: 64).
  - Developed deterministic retention scoring incorporating activation, priority, epistemic status, and unresolved status.
  - Protected pinned items and unresolved contradictions from eviction.
  - Provided serializable snapshot and restoration capabilities.
- **Phase D — Goal & Agenda Management:**
  - Implemented hierarchical goals with parent-child trees and dependency propagation (`src/cognition/goals.rs`).
  - Enforced typed completion criteria: goals cannot be declared complete without verifying explicit criteria (e.g. verified tool execution).
- **Phase E — Selective Attention & Prioritisation:**
  - Built `AttentionEngine` (`src/cognition/attention.rs`) with deterministic scoring based on readiness, contradiction resolution, base priority, uncertainty, and missing variables.
  - Enforced deterministic tie-breaking.
- **Phase F — Executive Controller:**
  - Implemented 10-phase discrete control cycle (`src/cognition/executive.rs`).
  - Enforced strict action allowlist, precondition checking, budget limits, timeouts, and rolling infinite loop detection.
- **Phase G — Metacognitive Monitoring:**
  - Implemented typed evaluation of goal progress, repeated failures, unverified calculations, and unresolved contradictions.
- **Phase H — Persistence & Resumption:**
  - Upgraded SQLite schema to version 2 (`src/store.rs`), adding `cognitive_state_json` and append-only `cognitive_events` table.
  - Verified cross-process state recovery and resumption without re-executing completed goals.
- **Phase I — Transparent Frontend Visibility:**
  - Updated research interface (`web/index.html`) to display active goals, working memory items, executive phase, cycle count, attention focus, and stop reasons.

---

## Future Development Horizons

### Milestone 1.4: Scholarly Evidence Retrieval & Semantic Memory
- Primary scientific literature integration:
  - OpenAlex scholarly API.
  - Crossref DOI metadata resolution.
  - PubMed Central open-access full-text retrieval.
- Semantic memory architecture:
  - Persistent scientific knowledge items with semantic embeddings.
  - Provenance-tracked citation grounding for `evidence_supported` epistemic claims.
- Note: External credentials (e.g. Zep, API keys) remain strictly optional; offline mock and SQLite storage will remain fully functional.

### Milestone 1.5: Procedural Learning & Formal Verification
- Formal theorem proving integration:
  - Lean 4 interactive theorem prover verification for mathematical derivations.
- Procedural memory:
  - Reusable problem-decomposition schemas distilled from validated investigations.
  - Template matching for recurring physical and mathematical problem topologies.

### Milestone 1.6: Interactive 3D Biomolecular Structure Exploration
- Interactive Mol* biomolecular viewer sidecar integration for PDB and mmCIF structural exploration.
- Residue-level contact distance verification.
