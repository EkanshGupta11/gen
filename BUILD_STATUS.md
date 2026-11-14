# Build status — Milestone 1.2 (implementation partial; blockers remain)

## Repository audit and preservation

- Preserved Git history and existing source/data. The active SQLite database and earlier milestone backup were inspected before changes; existing legacy schema is migrated additively by Rust with a new unique online backup made before alteration. Migration preservation is covered by a test using a temporary database.
- Existing local `.env` was checked without printing values. No model API key or model name is configured, so no real hosted inference call was attempted.
- Existing installed toolchain: Rust/Cargo 1.99.1, MSYS2/UCRT Python 3.14.6, Node 24. `.venv` is project-local and ignored. Rust `rustfmt`/Clippy components were installed in the existing toolchain.
- Added and locked `reqwest`, `rusqlite` (bundled SQLite + backup), `async-trait`, and supporting dependencies. Cargo registry access was intermittent; all subsequent builds use the local cache with `--offline --locked`.
- Attempted project-local install of NumPy, SciPy, SymPy, pandas, and matplotlib. The configured package index had no compatible binary distributions for this MSYS2/UCRT Python 3.14 interpreter. No native source build was attempted; these optional numerical dependencies are a documented blocker.

## Completed in this working tree

- Added a replaceable Rust `ModelProvider` trait, OpenAI-compatible asynchronous Chat Completions transport, explicit mock provider, response schema parsing with one bounded repair attempt, usage accounting, maximum calls/tokens, retries for rate limits/server failures, timeout/auth errors, and legacy token-field fallback only when an unsupported parameter is reported.
- Rust now creates, plans, schedules selected specialist requests, dispatches allowlisted calls, synthesizes, records epistemic statuses, and owns SQLite history. The Python worker no longer has investigation/model/network/database code.
- Added additive SQLite `user_version` migration and pre-migration online backup, plus create/update/list/search/get/delete behavior. Existing records retain their legacy result JSON.
- Added versioned (`1.0`) Rust-to-Python tool contracts, input validation for finite numbers and singular cases, standard-library deterministic arithmetic/regression/polynomial/statistics/projectile tools.
- Updated UI to display actual plan subproblems, specialist report text/status, tools/results/methods, verification, synthesis, uncertainty, and saved history.
- Updated `README.md` and `ARCHITECTURE.md` with current ownership, setup, decisions, and known limitations.

## Executed checks to date

- `cargo fmt --all` and `cargo fmt --check`: passed.
- `cargo build --offline --locked --target-dir work/cargo-run-2`: passed.
- `cargo check --offline --locked`: passed.
- `cargo test --offline --locked`: 15 passed, including provider fake-transport request/retry/auth/timeout/invalid-JSON recovery tests, SQLite legacy migration/backup and fresh-store persistence tests, API boundary tests, and an end-to-end mock investigation that executes the Python projectile worker and reads the saved investigation.
- `cargo clippy --offline --locked --all-targets -- -D warnings`: passed.
- Python syntax compilation and unit tests: passed (11 tests), including worker contract/version/error cases and invalid numeric inputs.
- Extracted frontend inline JavaScript `node --check`: passed.
- `git diff --check`: passed (only Git's expected line-ending notices on Windows).
- Actual server startup reached the bind step, but this environment rejects loopback bind/connect operations with Windows socket error 10048. Direct health/investigation HTTP tests therefore could not run; in-process Axum request tests pass. No browser-to-backend success is claimed.
- Startup migration ran against the existing active DB. `user_version=1`; the legacy schema was extended; row count remained 0. `data/neurogenesis.pre-m1.2.bak.sqlite3` was created, and the earlier `.pre-milestone-1.1` backup remains untouched.

## Remaining before milestone completion

- Obtain a Python runtime/platform with compatible scientific package wheels, then implement and test SymPy/SciPy/NumPy-based scientific tools.
- Implement bounded parallel specialist scheduling and explicit disagreement/contradiction analysis.
- Add genuinely independent calculation verification (current computed results are honestly marked calculated and `independently_checked=false`).
- Exercise provider and investigation HTTP behavior via a permitted local server environment. The current managed environment blocks sockets, so this remains unverified outside in-process Router tests.
- Add a subprocess/application-restart persistence test, rather than only reopening a Store instance in the same process.
- Run the project scripts once more and commit the finished milestone after the remaining capabilities above are complete.

## Known gaps

- No real hosted inference test: credentials are absent. There is no scholarly evidence retrieval in this milestone, so external scientific claims cannot become evidence-supported.
- Specialist calls are currently serial. Cross-domain structured disagreement aggregation, independent deterministic recalculation of tool outputs, and richer computation handoff remain incomplete.
- Current Python tools are standard-library implementations. SymPy/SciPy/NumPy tool suite could not be installed in the available Python environment.
- Zep, literature adapters, uploads, Lean, and 3D visualization remain later milestones.
