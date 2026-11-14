# NeuroGenesis AI architecture

## Repository and environment audit

The repository was an uninitialized source workspace with a local SQLite database at `data/neurogenesis.sqlite3`. Its table was inspected. A test-fixture isolation defect was found and fixed; a complete pre-cleanup backup is retained at ignored `data/neurogenesis.pre-milestone-1.1.sqlite3`. The 17 rows in the active database all matched known test prompts and were removed from active history; no other records existed, and the schema was not migrated. Rust 1.99/Cargo 1.99, Python 3.14.6, Node 24, and uv are present. A project-local Python virtual environment is now in `.venv` (ignored by Git). Cargo's local cache contains Axum, Tokio, Serde, Tower HTTP, and their transitive crates, so the Rust server could be upgraded and locked without network access. DNS resolution to crates.io failed; PyPI and npm registry DNS checks were denied by the environment. Official documentation remains reachable through the documentation browser.

## Milestone 1.1 decisions

- Axum/Hyper owns HTTP framing, Content-Length handling, routing, and HTTP method semantics. Tokio owns asynchronous I/O, timeouts, and graceful Ctrl+C shutdown.
- Serde typed request contracts validate JSON at the Rust boundary. Request bodies have a configurable limit; application requests and Python worker calls are bounded by timeouts.
- Tower HTTP supplies request-size limits and structured HTTP tracing. The UI is same-origin, so cross-origin access is disabled instead of allowing every origin.
- The scientific worker is isolated as a child process receiving one JSON payload on stdin and a versioned route argument. Rust maps worker failures to HTTP status codes.
- No database migration was run. SQLite remains on the existing schema behind the current Python memory implementation because rusqlite was absent from the offline Cargo cache. Moving memory and orchestration into Rust remains follow-up architecture work.
- The Python environment uses a local `.venv`; scientific third-party packages are not installed because package registries are unreachable and the current worker has a standard-library-only fallback.

## Current component ownership

- **Rust:** HTTP API, request validation, bounded worker dispatch, service logs, and graceful shutdown.
- **Python:** current investigation orchestration, optional OpenAI-compatible HTTP call, scientific tool implementations, and SQLite history adapter. This is a known transition state; the target is Rust orchestration/provider/memory interfaces with Python limited to computation.
- **Frontend:** dependency-free same-origin browser UI served from the Rust binary.

## Request flow

Browser → Axum API and typed contracts → bounded Python worker process → scientific computation / current investigation and SQLite adapter → JSON response.

There is no network CORS policy because the production UI is served from the API origin. Hosted inference is only attempted when explicitly configured. No secrets are compiled into the UI.
