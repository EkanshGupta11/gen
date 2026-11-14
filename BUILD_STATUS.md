# Build status — Milestone 1.2: Local Development Environment & End-to-End Validation

## Repository audit and preservation

- Preserved Git history and user data. Working branch `env-validation` created from `main`.
- Real user SQLite database (`data/neurogenesis.sqlite3`) and pre-migration backups (`neurogenesis.pre-m1.2.bak.sqlite3`, `neurogenesis.pre-milestone-1.1.sqlite3`) remain intact. All automated persistence tests run against isolated temporary databases.
- Existing local `.env` checked without logging secrets. Hosted credentials (`MODEL_API_KEY`) remain unconfigured, so offline mock mode is active for safe deterministic testing.

## Local Environment & Python Toolchain (Completed)

- **Astral uv installed & active:** uv 0.12.10 (x86_64-pc-windows-msvc).
- **Managed CPython 3.13 installed:** CPython 3.13.15 installed via uv at `C:\Users\Ekansh\AppData\Roaming\uv\python\cpython-3.13.15-windows-x86_64-none\python.exe`.
- **Project-local virtual environment:** Created dedicated `.venv-cpython313` virtual environment using standard CPython 3.13, isolating the project from the previous MSYS2/UCRT Python 3.14 environment.
- **Scientific packages installed & verified:**
  - `numpy==2.5.4`
  - `scipy==1.18.1`
  - `sympy==1.14.0`
  - `pandas==3.1.1`
  - `matplotlib==3.11.2`
  - Verified package imports and versions via `.venv-cpython313/Scripts/python.exe`.
- **Reproducible dependency metadata & scripts:**
  - Created `requirements.txt` locking scientific dependencies.
  - Updated `scripts/setup.ps1` to detect `uv`, provision CPython 3.13, create `.venv-cpython313`, and install from `requirements.txt`.
  - Updated `scripts/dev.ps1`, `scripts/test.ps1`, and `scripts/check.ps1` to resolve `.venv-cpython313/Scripts/python.exe`.
  - Updated `.env` and `.env.example` with `PYTHON_EXECUTABLE=.venv-cpython313/Scripts/python.exe`.
  - Updated `.gitignore` to ignore `.venv-cpython313/`.

## Local Application Testing & HTTP Communication (Completed)

- **Configured HTTP Port:** Inspected `.env` (`HOST=127.0.0.1`, `PORT=8787`).
- **Port Collision Resolution:** Identified Windows socket error 10048 was caused by a prior instance of `neurogenesis.exe` (PID 36412) holding port 8787. Safely terminated the orphaned process; loopback bind succeeded without OS restrictions.
- **Live HTTP Health Checks:**
  - `GET /health` -> HTTP 200 `{"status":"ok","contract_version":"1.0","mode":"offline-capable"}`
  - `GET /api/health` -> HTTP 200 `{"status":"ok","contract_version":"1.0","mode":"offline-capable"}`
  - `POST /api/settings` -> HTTP 200 `{"hosted_ready":false,"max_tokens_per_request":1200,"memory_provider":"sqlite","model_name":"offline-mock","model_provider":"mock"}`
- **Live Investigation Submission:**
  - `POST /api/investigate` -> HTTP 200, successfully orchestrated planning, specialist routing, executed Python calculation tool (`projectile` returning `range_m = 38.33`), synthesized findings, and persisted record.
- **Browser-to-Backend End-to-End Verification:**
  - Executed automated Chrome DevTools Protocol (CDP) test (`scripts/test_browser_e2e.js`) with headless Chrome.
  - Verified frontend loaded from `http://127.0.0.1:8787/`.
  - Verified badge dynamically updated to "Offline mock mode" after fetching `/api/settings`.
  - Submitted an investigation via the UI form; verified browser DOM received and rendered research plan, specialist reports, tool outputs, and synthesis.
  - Verified sidebar history populated with saved investigations.

## Cross-Process Database Persistence & Recovery (Completed)

- **Automated recovery test (`scripts/test_persistence_recovery.py`):**
  - Generated isolated temporary SQLite database in temp directory.
  - Launched server process 1 and submitted an investigation via HTTP POST.
  - Verified investigation completion and persistence in SQLite.
  - Terminated server process 1.
  - Launched independent server process 2 pointing to the same database.
  - Queried `/api/history` and `GET /api/investigations/{id}` on server process 2; verified exact ID, calculation output (`range_m = 38.33`), and synthesis survived process termination.
  - Terminated server process 2 and cleanly removed temporary files.
  - Verified zero modification or deletion of real user data.

## Executed Checks and Test Suite Results

- `cargo fmt --check`: passed (0 formatting violations).
- `cargo clippy --locked --all-targets -- -D warnings`: passed (0 warnings, 0 errors).
- `cargo check --locked --offline`: passed.
- `cargo test --locked --offline`: 15 passed, 0 failed.
- Python unit test suite (`unittest discover -s engine` with CPython 3.13): 11 passed, 0 failed.
- Frontend inline JavaScript syntax check (`node --check`): passed.
- Cross-process database recovery test (`scripts/test_persistence_recovery.py`): passed.
- Browser end-to-end verification (`scripts/test_browser_e2e.js`): passed.

## Remaining Before Next Milestone

- Implement bounded parallel specialist scheduling (currently serial).
- Implement explicit cross-specialist disagreement/contradiction aggregation.
- Implement independent calculation verification (current computed results are marked calculated with `independently_checked=false`).
- Integrate real hosted model inference once API credentials are provided.
- Integrate scholarly evidence retrieval (e.g. OpenAlex/Crossref) to enable `evidence_supported` claims.
- Zep long-term memory, document ingestion, Lean theorem proving, and 3D molecular visualization belong to future milestones.
