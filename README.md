# NeuroGenesis AI

An offline-capable scientific investigation workspace. Rust owns investigation planning, selected specialist requests, hosted-model communication, scientific-tool dispatch, verification coordination, and persistent investigation history. Python runs deterministic calculations. The browser shows saved plans, specialist outputs, calculations, epistemic status, and synthesis.

## Windows quick start

Requirements: stable Rust with `rustfmt` and `clippy`, Python 3.11 or later, and a modern browser. Node.js is not required for the current dependency-free frontend.

1. Run `scripts/setup.ps1` once.
2. Keep `MODEL_PROVIDER=mock` for offline mode, or configure `MODEL_PROVIDER=openai_compatible`, `MODEL_BASE_URL`, `MODEL_API_KEY`, and `MODEL_NAME` in local `.env`.
3. Run `scripts/dev.ps1` and open http://127.0.0.1:8787.

The mock provider exercises the workflow without credentials but explicitly labels all outputs as mock and performs no hosted scientific reasoning. Deterministic tool calls still run locally. Hosted questions are sent to the configured service; do not submit sensitive material unless its data handling is acceptable. API keys stay server-side.

## Current capabilities

- Stable Rust/Axum API and orchestrator with typed request/response structures.
- Configurable OpenAI-compatible hosted provider, structured JSON parsing, bounded retries, request/token limits, timeout/rate-limit handling, and offline mock transport.
- Five specialist instructions and structured outputs. Only selected specialists are called, with an independent provider call per role.
- Versioned Python worker with arithmetic, power-law fit, polynomial evaluation, statistics, and ideal projectile motion.
- Rust-owned SQLite history with additive migrations, pre-migration backup, investigation retrieval, search, and deletion.
- Explicit epistemic status; unsupported claims are not marked evidence-supported or verified.
- Responsive, same-origin research workspace rendering actual API results.

## Checks

Run `scripts/check.ps1` for formatting, Rust checks and Python syntax; `scripts/test.ps1` runs Rust and Python tests. The offline Rust integration test performs planning, specialist routing, a real Python projectile computation, synthesis, and persistence.

## Limitations

Live model inference is untested because this environment has no configured key. The existing MSYS2/UCRT Python 3.14 environment had no compatible prebuilt wheels for NumPy/SciPy/SymPy/pandas/matplotlib from its configured package index, so the current tools stay standard-library based. Specialist calls are serial and there is no live scholarly retrieval yet. Zep, uploads, Lean, and 3D visualization are later milestones. See [ARCHITECTURE.md](ARCHITECTURE.md), [DEVELOPMENT.md](DEVELOPMENT.md), and [BUILD_STATUS.md](BUILD_STATUS.md).
