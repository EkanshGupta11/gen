# Build status — Milestone 1.1

## Completed

- Audited the existing Rust API, Python application and tests, frontend, scripts, manifests, dependencies, and SQLite schema. During testing I found that the old Python module cached `DATABASE_PATH`, causing fixtures to leak into the existing database. I backed up the database to ignored `data/neurogenesis.pre-milestone-1.1.sqlite3`, verified all 17 rows matched the two exact test prompts, then removed only those test fixtures from the active database. No other records existed, the schema was not migrated, and subsequent tests left the active database empty.
- Replaced the single-read raw TCP server with Axum/Hyper and Tokio. Added Serde request types, bounded request bodies, bounded Python worker calls, a global request timeout, proper route/method behavior, JSON errors, structured tracing, same-origin-only access, and graceful Ctrl+C shutdown.
- Added worker exit-code mapping for invalid requests, provider failures, and unexpected failures. Python exceptions no longer report process success.
- Added seven Rust API test functions covering health/contracts, malformed JSON, oversize body, invalid question, unsupported tool, unavailable worker, timeout, and successful dispatch to the real Python arithmetic worker.
- Created project-local `.venv` with Python 3.14.6 and pip; setup/dev/check/test PowerShell scripts use it where available. Setup script completed successfully.
- Initialized Git on `main`; initial baseline commit: `a1f7191`. `.gitignore` excludes `.env`, databases/backups, virtual environments, caches, binaries, and build output.

## Actual checks

- `cargo build --locked` through `scripts/setup.ps1`: passed.
- `cargo check --offline --locked`: passed through `scripts/check.ps1`.
- `cargo test --locked --offline`: passed; 7 Rust tests, including real Python worker dispatch.
- Python syntax compilation and unit tests passed in `.venv`: 11 tests, including nonzero worker exit codes and SQLite persistence across separate worker processes.
- Node `--check` on the inline frontend script: passed.
- `cargo fmt` and `cargo clippy` could not run because `cargo-fmt.exe` and `cargo-clippy.exe` are missing. The `rustfmt` command is also only a rustup shim for the missing component. We did not retry installs because DNS to crates.io is unavailable.
- The Axum listener started, but HTTP health smoke testing was blocked because the environment denied access to a loopback client socket. In-process Axum `Router::oneshot` request tests pass. Browser automation and npm dependencies are not installed.
- crates.io lookup failed with DNS resolution error. PyPI and npm registry DNS access was denied. Cached Cargo crates allowed dependencies to build offline. Official technical documentation is reachable in the browser.

## Remaining work

Rust currently owns HTTP and its typed boundary but delegates the existing investigation orchestration, optional provider transport, and SQLite operations to the Python application. The next architecture milestone should move those interfaces into Rust while retaining Python as a computation-only worker. Hosted inference was not tested with credentials. Retry/backoff, rate-limit-specific errors, scholarly retrieval, Zep, scientific third-party libraries, differential equations, Lean, uploads, and 3D visualization remain incomplete. No claim is made that these capabilities are complete.
