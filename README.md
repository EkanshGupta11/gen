# NeuroGenesis AI

An offline-first scientific investigation workspace with deterministic local tools, scientific role selection, epistemic labels, and persistent SQLite history. Mock activity is explicitly labeled as role selection; it does not claim that an agent performed an analysis.

## Quick start (Windows PowerShell)

Requirements: stable Rust/Cargo, Python 3.11+, and a modern browser. Node is not required for the dependency-free UI. Run `./scripts/setup.ps1`, then `./scripts/dev.ps1`, and open http://127.0.0.1:8787. Setup creates a project-local `.venv`; local databases and secrets are excluded from Git.

To configure the optional hosted provider, set `MODEL_PROVIDER=openai_compatible`, `MODEL_BASE_URL`, `MODEL_API_KEY`, and `MODEL_NAME` in local `.env`. Questions are sent to that provider when configured. Do not submit sensitive research unless the provider's data handling is acceptable. This network-dependent path has not been tested with credentials in the current environment.

## Current features

- Axum/Tokio Rust HTTP API with typed requests, request-size limits, worker timeouts, structured errors, request tracing, and graceful shutdown.
- Distinct mathematics, physics, chemistry, biology, and medicinal-science roles. Offline mode reports role selection without claiming completed specialist analysis.
- Python tools for arithmetic, power-law fitting, polynomial evaluation, descriptive statistics, and ideal projectile motion.
- Local SQLite history with search and restart persistence.
- Epistemic labels for calculations and hypotheses, plus a responsive same-origin research UI.

Rust currently serves the typed API and dispatches to the existing Python application, which still owns investigation orchestration, hosted-provider transport, and SQLite operations. Moving these boundaries into Rust remains a follow-up milestone. See [ARCHITECTURE.md](ARCHITECTURE.md), [DEVELOPMENT.md](DEVELOPMENT.md), and the `docs/` directory for design details and known limitations.
