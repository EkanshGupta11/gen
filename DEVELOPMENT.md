# Development Guide

## Windows Quick Start

Requirements:
- Stable Rust (1.80+ with `rustfmt` and `clippy`)
- Astral `uv` with managed CPython 3.13
- Node.js (v20+ for CDP automated browser testing; not required for production serving)
- Google Chrome (for automated CDP browser tests)

1. Provision Environment & Dependencies:
   ```powershell
   ./scripts/setup.ps1
   ```
   This automatically provisions standard CPython 3.13 via `uv`, creates `.venv-cpython313`, installs `requirements.txt` (`numpy`, `scipy`, `sympy`, `pandas`, `matplotlib`), and compiles the Rust application.

2. Configure Environment:
   Copy `.env.example` to `.env` if not already present. Default configuration runs in offline mock mode:
   ```env
   HOST=127.0.0.1
   PORT=8787
   DATABASE_PATH=data/neurogenesis.sqlite3
   PYTHON_EXECUTABLE=.venv-cpython313/Scripts/python.exe
   MODEL_PROVIDER=mock
   SPECIALIST_CONCURRENCY=3
   ```

3. Launch Development Server:
   ```powershell
   ./scripts/dev.ps1
   ```
   Open http://127.0.0.1:8787 in your browser.

---

## Configuration Reference

| Environment Variable | Description | Default | Allowed Values |
| :--- | :--- | :--- | :--- |
| `HOST` | Bind address for HTTP server | `127.0.0.1` | Valid IPv4/IPv6 string |
| `PORT` | Listening port | `8787` | Valid TCP port (1–65535) |
| `DATABASE_PATH` | Path to SQLite database | `data/neurogenesis.sqlite3` | File path |
| `PYTHON_EXECUTABLE` | Path to Python 3.13 interpreter | `.venv-cpython313/Scripts/python.exe` | File path |
| `MODEL_PROVIDER` | Language model provider | `mock` | `mock`, `openai_compatible` |
| `SPECIALIST_CONCURRENCY` | Maximum concurrent specialist tasks | `3` | `1` to `5` (`1` forces serial mode) |
| `MODEL_BASE_URL` | Base URL for hosted provider | None | Valid HTTP/HTTPS URL |
| `MODEL_API_KEY` | API key for hosted provider | None | String |
| `MODEL_NAME` | Model identifier | `offline-mock` | String |
| `MODEL_TIMEOUT_SECONDS` | Per-request hosted model timeout | `30` | Seconds (integer) |
| `MAX_TOKENS_PER_REQUEST` | Token budget per model call | `1200` | Tokens (integer) |

---

## Test Suites & Quality Verification

Run all test suites via:
```powershell
./scripts/test.ps1
```

This script executes the complete validation pipeline:
1. **Rust Test Suite:** `cargo test --locked --offline` (26 unit and integration tests passing).
2. **Python Test Suite:** `.venv-cpython313/Scripts/python.exe -m unittest discover -s engine -p "test_*.py" -v` (11 unit tests passing).
3. **Frontend JS Syntax:** Validates `web/index.html` inline JavaScript via `node --check`.
4. **Cross-Process Persistence Recovery:** `python scripts/test_persistence_recovery.py` validates SQLite persistence across process restarts, tool outputs, and verification records using isolated temporary databases.
5. **Browser End-to-End Test:** `node scripts/test_browser_e2e.js` uses native Chrome DevTools Protocol to verify UI rendering, investigation submission, and DOM status badges (`.pill.pill-verified`).

### Code Quality & Static Analysis:
```powershell
./scripts/check.ps1
```
Enforces:
- `cargo fmt --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo check --locked`
- Python syntax compilation checks

---

## Epistemic Integrity Guidelines

1. **Calculated vs. Verified:** A calculation executed by a tool is `calculated`. It is only marked `verified` if an independent checker in Rust validates the output against closed-form physical equations, exact arithmetic, or statistical invariants.
2. **Models Cannot Self-Verify:** A model must never be allowed to assign `verified` or `evidence_supported` to its own output.
3. **Test Database Safety:** Never run tests against real user data in `data/neurogenesis.sqlite3`. Automated tests must use temporary SQLite databases.
