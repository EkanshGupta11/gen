# Development

## Windows quick start

From PowerShell in the repository, run `./scripts/setup.ps1` once, configure `.env` if needed, then run `./scripts/dev.ps1` and open http://127.0.0.1:8787. Setup creates `.venv` without changing system Python and builds from `Cargo.lock`. The launch script prefers `.venv`'s interpreter and loads `.env` into that process only.

Run `./scripts/check.ps1` for formatting availability, Rust compilation, and Python syntax checks. Run `./scripts/test.ps1` for Rust API tests and Python tests. The server supports `HOST`, `PORT`, `MAX_REQUEST_BYTES`, `WORKER_TIMEOUT_SECONDS`, and `PYTHON_EXECUTABLE`.

## Milestone 1.1 checks and environment

The actual offline Cargo checks and test suite pass using cached Axum/Tokio/Serde dependencies. The Python worker tests pass in Python 3.14.6. `rustfmt` and Clippy components are missing from the active Rust toolchain (`cargo fmt` reports missing `cargo-fmt.exe`; `cargo clippy` reports missing `cargo-clippy.exe`). The repository DNS checks failed for crates.io and were denied for PyPI and npm registries. The current Cargo lockfile is resolved locally; if a dependency is not cached on another machine, normal Cargo setup will need registry access.

The application can be started, but this environment blocks loopback HTTP client access; API behavior is covered with in-process Axum request tests instead. Browser automation packages are not installed, and npm registry access is unavailable.

## Known limitations

- Rust currently provides the typed HTTP gateway and worker boundary, while investigation orchestration, hosted provider transport, and SQLite remain in `engine/app.py`. Moving those interfaces into Rust is the next architecture milestone.
- The hosted provider path has not been exercised with credentials. It does not yet implement retry/backoff or provider-specific status mapping.
- OpenAlex/Crossref retrieval, Zep, NumPy/SciPy/SymPy computations, differential equation solving, document upload, Lean, and 3D visualization are incomplete.
- Mock role activity is shown as `role_selected`, not as completed analysis.
- Python computations are standard-library only in the present environment. Never interpret an idealized calculation as experimental or clinical validation.
