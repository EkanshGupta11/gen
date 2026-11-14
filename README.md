# NeuroGenesis AI

An offline-capable scientific investigation workspace. Rust owns investigation planning, selected specialist requests, hosted-model communication, scientific-tool dispatch, verification coordination, and persistent investigation history. Python provides deterministic calculations. The browser displays saved plans, specialist outputs, calculations, epistemic status, and synthesis.

## Windows Quick Start

Requirements: stable Rust with `rustfmt` and `clippy`, Astral `uv` with Python 3.13, and a modern browser. Node.js is not required for production serving (a static dependency-free frontend is compiled into the binary).

1. Run `scripts/setup.ps1` once to provision the Python 3.13 scientific virtual environment (`.venv-cpython313`), install dependencies, and build the Rust application.
2. Keep `MODEL_PROVIDER=mock` in `.env` for offline mode, or configure `MODEL_PROVIDER=openai_compatible`, `MODEL_BASE_URL`, `MODEL_API_KEY`, and `MODEL_NAME`.
3. Run `scripts/dev.ps1` and open http://127.0.0.1:8787 in your browser.

The mock provider exercises the workflow without credentials, explicitly labeling all outputs as mock and performing no hosted scientific reasoning. Deterministic scientific tool calls execute locally.

## Current Capabilities

- Stable Rust/Axum API and orchestrator with typed request/response contracts (`1.0`).
- Configurable OpenAI-compatible hosted provider with structured JSON schema parsing, bounded retries, token budgeting, and offline mock transport.
- Five specialist roles with allowlisted scientific tool dispatch.
- Versioned Python worker running in an isolated CPython 3.13 virtual environment with pre-installed scientific packages (`numpy`, `scipy`, `sympy`, `pandas`, `matplotlib`).
- Standard-library arithmetic, power-law regression, polynomial evaluation, descriptive statistics, and ideal projectile motion calculations.
- Rust-owned SQLite history with additive schema migrations, pre-migration backup, search, retrieval, and cross-process persistence recovery.
- Explicit epistemic statuses: unsupported claims remain hypotheses, and computations are distinguished from experimental validations.
- Responsive same-origin research workspace verified end-to-end with live browser and HTTP client tests.

## Checks & Verification

- `scripts/check.ps1`: runs `cargo fmt --check`, `cargo clippy`, `cargo check`, and Python compilation checks.
- `scripts/test.ps1`: runs the complete test suite including:
  1. Rust unit & offline integration tests (15 tests).
  2. Python calculation worker unit tests (11 tests).
  3. Frontend JavaScript syntax verification.
  4. Automated cross-process database persistence recovery test.
- `scripts/test_browser_e2e.js`: performs automated browser interaction via Chrome DevTools Protocol (CDP).

## Limitations & Next Steps

- Live hosted inference is untested pending user-provided API credentials.
- Specialist execution is bounded and role-isolated but currently runs serially.
- Independent calculation verification remains incomplete (`independently_checked=false`).
- Scholarly retrieval (OpenAlex/Crossref), Zep memory, Lean theorem proving, and 3D visualization belong to subsequent milestones. See [ARCHITECTURE.md](ARCHITECTURE.md), [DEVELOPMENT.md](DEVELOPMENT.md), and [BUILD_STATUS.md](BUILD_STATUS.md).
