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

### Milestone 1.3: Cognitive Control Foundation
- Implemented modular cognitive state, bounded working memory, hierarchical goals, selective attention, executive controller, and schema version 2 SQLite persistence.

### Milestone 1.3.1: Cognitive Runtime Integration & Epistemic Integrity
- **Production Cognitive Runtime:** Implemented `CognitiveRuntime` and `AsyncActionExecutor`, transferring orchestration authority from passive pipelines to active discrete cognitive control cycles.
- **Real Scientific Computation Execution:** Replaced placeholder mocking in the controller with real asynchronous execution via `worker::invoke` in CPython 3.13 against `engine/app.py`.
- **Non-Bypassable Verification:** Enforced mandatory independent Rust verification checkers for all deterministic tool executions, preventing self-certification of epistemic statuses.
- **Evidence-Based Contradiction Arbitration:** Implemented structured `ContradictionResolution` requiring defensible verified evidence to resolve contradiction records.
- **Honest Action Execution Status:** Introduced `ActionExecutionStatus` ontology (`Succeeded`, `Failed`, `Contradicted`, `Uncertain`, `Blocked`, `Unsupported`, `AwaitingHumanInput`, `Cancelled`), ensuring unsupported actions never report false success.
- **Goal Completion Criteria Scoping & Failure Isolation:** Strictly scoped completion criteria to specific subproblem goal IDs. Subproblem failures cleanly isolate downstream dependencies without aborting overall investigation synthesis.
- **State Recovery & Budget Preservation:** Fully integrated SQLite snapshot and append-only event persistence, restoring historical budgets (`total_actions_executed`) and rolling loop detection histories on restart.
- **Test Suite Expansion:** Expanded test suite to 42 passing Rust unit/integration tests and verified end-to-end browser DOM rendering.

### Milestone 1.3.2: Runtime Reliability Closure
- **Runtime Error Propagation:** Captured `runtime.run().await` return values; propagated failures cleanly to ensure runtime errors cannot result in `InvestigationStatus::Completed`.
- **Atomic Cognitive Persistence & Recovery:** Implemented single-transaction snapshot and event updates with ACID rollback; pending events in memory are never cleared until durably committed to SQLite.
- **Honest Terminal Status:** Added `InvestigationStatus::Interrupted` and implemented exhaustive `StopReason` outcome mapping so budget exhaustion, timeouts, deadlocks, and cancellations never silently report success.
- **Specialist Provenance Preservation:** Removed hardcoded `Specialist::Mathematics` tool attribution; dynamically resolved initiating specialists based on goal completion criteria, descriptions, and allowlists.
- **Bounded Concurrency Scheduling:** Restored parallel execution for independent ready subproblems via `max_concurrency`, with sequential state commitment ensuring executive control and state consistency.
- **Scientific Contradiction Arbitration:** Enforced deep comparability checks for proposition identity, physical assumptions (e.g. ideal vacuum vs air resistance), conditions, and units; multiple conflicting verified items route to `RequiresHumanReview`.
- **Verified Test Suite Expansion:** Expanded Rust test suite to 50 passing unit/integration tests, 11 Python tests, cross-process persistence recovery, and headless Chrome browser E2E verification.

### Milestone 1.3.2.1: Executive Control Hardening & Live Model Validation
- **Authoritative Batch Preparation Lifecycle:** Implemented `ExecutiveController::step_prepare_batch(max_batch)` ensuring all concurrent actions pass through cycle limits, timeouts, action budget reservation, and loop detection before dispatch.
- **Bidirectional Dependency Validation:** Prevented subproblems with mutual or sequential dependencies from entering concurrent execution batches.
- **Worker Mutation Isolation:** Ensured concurrent worker tasks operate on isolated state clones and only merge knowledge items strictly scoped to their assigned goal.
- **Live Gemini Provider Integration:** Validated OpenAI-compatible Gemini endpoints against live Google API with safe token budgeting, exponential backoff, and credential security.
- **Verified Test Suite Expansion:** Expanded Rust test suite to 55 passing unit/integration tests.

### Milestone 1.3.2.2: Deterministic Execution & Scientific Evidence Schema Closure (Completed Milestone)
- **Deterministic Concurrent Execution:** Eliminated out-of-order state commits by assigning stable batch indices `(0..batch.len())`, collecting results in indexed slots, and committing state sequentially in strict original batch order.
- **Fail-Safe Task Panic & Cancellation Handling:** Explicitly handled `JoinError` without discarding sibling task results, producing deterministic failed action results.
- **Canonical Scientific Evidence Schema:** Strictly enforced explicit tool/operation identity, deep input parameter comparison ($10^{-4}$ tolerance), explicit units with standard suffix inference, non-empty explicit assumptions, and output measured quantity key identity.
- **Fail-Closed Contradiction Arbitration:** Incomparable, unaligned, or missing metadata routes automatically to `RequiresHumanReview`.
- **Objective & Terminal Status Semantics:** Primary synthesis objectives govern investigation completion; failed synthesis objectives never report `Completed`, and cancelled investigations remain `Interrupted`.
- **Verified Test Suite Expansion:** Expanded Rust test suite to 59 passing unit/integration tests, 11 Python tests, cross-process persistence recovery, and headless Chrome browser E2E verification.

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
