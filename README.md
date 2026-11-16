# NeuroGenesis AI

An offline-capable scientific intelligence workspace. Rust owns investigation planning, internal cognitive control, selected specialist requests, hosted-model communication, scientific-tool dispatch, independent verification coordination, and persistent investigation history. Python provides deterministic calculations. The browser displays saved plans, specialist outputs, internal cognitive control state, working memory, calculations, epistemic status, and synthesis.

## Windows Quick Start

Requirements: stable Rust with `rustfmt` and `clippy`, Astral `uv` with Python 3.13, and a modern browser. Node.js is not required for production serving (a static dependency-free frontend is compiled into the binary).

1. Run `scripts/setup.ps1` once to provision the Python 3.13 scientific virtual environment (`.venv-cpython313`), install dependencies, and build the Rust application.
2. Keep `MODEL_PROVIDER=mock` in `.env` for offline mode, or configure `MODEL_PROVIDER=openai_compatible`, `MODEL_BASE_URL`, `MODEL_API_KEY`, and `MODEL_NAME`.
3. Optionally configure `SPECIALIST_CONCURRENCY=3` (values 1–5; 1 forces serial execution).
4. Run `scripts/dev.ps1` and open http://127.0.0.1:8787 in your browser.

The mock provider exercises the workflow without credentials, explicitly labeling all outputs as mock and performing no hosted scientific reasoning. Deterministic scientific tool calls execute locally and are verified by Rust.

## Current Capabilities

- **Cognitive Control Foundation (Milestone 1.3):**
  - *Bounded Working Memory Engine:* Configurable capacity (default: 64) with deterministic retention scoring, activation decay, retrieval reinforcement, and strict protection for pinned items and unresolved contradictions.
  - *Hierarchical Goal & Agenda Management:* Directed goal trees with dependency resolution, preconditions, and strictly verified completion criteria. Tasks are never marked complete solely because an agent asserts it.
  - *Selective Attention & Prioritisation:* Deterministic attention engine scoring goals on readiness, contradiction resolution, base priority, uncertainty, and missing information with deterministic tie-breaking.
  - *10-Phase Executive Controller:* Discrete loop (`Observe -> UpdateState -> EvaluateGoals -> Attention -> SelectAction -> ValidatePreconditions -> ExecuteAction -> EvaluateOutcome -> UpdateWorkingMemory -> MetacognitiveEvaluation -> ContinueOrStop`). Features rolling loop detection, cycle/action budgets, and deadlock handling.
  - *Metacognitive Monitoring:* Typed health evaluations evaluating progress, repeated failures, unverified calculations, and contradictions.
- **Strict Epistemic Classification:** Rigorous separation of claims into six mutually exclusive statuses: `verified`, `calculated`, `evidence_supported`, `hypothesis`, `uncertain`, and `contradicted`. A model is never permitted to self-assign `verified` status.
- **Independent Scientific Verification:** Deterministic closed-form checkers natively implemented in Rust (`src/verification.rs`):
  - *Projectile Motion:* Closed-form Newtonian kinematics ($R = v^2\sin(2\theta)/g$, $T = 2v\sin\theta/g$, $H = v^2\sin^2\theta/(2g)$), boundary validations ($\theta=0^\circ, 90^\circ$), input constraints ($v \ge 0, g > 0$), and contradiction detection.
  - *Arithmetic:* Exact integer and floating-point verification for addition, subtraction, multiplication, and division with zero-division safety.
  - *Descriptive Statistics:* Sample mean, Bessel-corrected standard deviation ($N-1$), min, max, and count verification.
  - *Power-Law Fitting Consistency:* Verifies consistency with least-squares normal equations; empirical fits strictly retain `calculated` status and are never claimed as `verified`.
- **Bounded Asynchronous Specialist Scheduling:** Dynamic DAG dependency resolution using Tokio `JoinSet` bounded by `SPECIALIST_CONCURRENCY` (1..=5, default 3). Includes serial execution mode (`SPECIALIST_CONCURRENCY=1`), dependency context propagation, dependency failure surfacing, and failure isolation.
- **Five Scientific Specialist Roles:** Mathematics, Physics, Chemistry, Biology, and Medicinal Science with role-isolated allowlisted tool dispatch.
- **Hosted Model Client & Bounded Retries:** Configurable OpenAI-compatible provider with structured JSON schema enforcement, token budget tracking, bounded retries on 500/503 errors, and offline mock transport.
- **Isolated Python Worker:** CPython 3.13 virtual environment (`.venv-cpython313`) with pre-installed scientific libraries (`numpy`, `scipy`, `sympy`, `pandas`, `matplotlib`).
- **Rust-Owned SQLite Persistence:** Additive schema migrations (version 2) with automated pre-migration backup, append-only cognitive events log, and cross-process database recovery and resumption.
- **Modern Responsive Frontend:** Single-page application rendering real-time investigation progress, cognitive control state, working memory summary, semantic epistemic badges, and interactive history.

## Verification & Test Suite

- `scripts/check.ps1`: runs `cargo fmt --check`, `cargo clippy`, `cargo check`, and Python compilation checks.
- `scripts/test.ps1`: runs the complete test suite including:
  1. Rust unit & offline integration tests (**38 tests**, 0 failures).
  2. Python calculation worker unit tests (**11 tests**, 0 failures).
  3. Frontend JavaScript syntax verification.
  4. Automated cross-process database persistence recovery test.
  5. Automated browser end-to-end verification (`scripts/test_browser_e2e.js`) using Chrome DevTools Protocol (CDP).

## Documentation Links

- [COGNITIVE_ARCHITECTURE.md](COGNITIVE_ARCHITECTURE.md) — Comprehensive specification of the Milestone 1.3 cognitive control subsystem.
- [ARCHITECTURE.md](ARCHITECTURE.md) — System boundaries, components, and dataflow.
- [DEVELOPMENT_ROADMAP.md](DEVELOPMENT_ROADMAP.md) — Complete roadmap and milestone trajectories.
- [NEUROGENESIS_MASTER_CONTEXT.md](NEUROGENESIS_MASTER_CONTEXT.md) — Master context and operating guidelines.
- [BUILD_STATUS.md](BUILD_STATUS.md) — Detailed build, test, and milestone verification records.
