# NeuroGenesis AI architecture

## Milestone 1.2 ownership

- **Rust (Axum/Tokio):** HTTP API, validation, investigation lifecycle and orchestration, structured plan/specialist/synthesis provider calls, specialist selection, tool allowlist, verification coordination, and SQLite history.
- **Python:** isolated deterministic computation worker only. It accepts JSON contract version `1.0` over stdin, runs a named allowlisted calculation, and returns a versioned JSON result. It has no model credentials, network client, investigation history, or database code.
- **Hosted provider:** replaceable `ModelProvider` abstraction. `openai_compatible` uses the documented Chat Completions JSON mode and current `max_completion_tokens` field, with bounded retries and legacy token-field fallback for explicit unsupported-parameter responses. `mock` is an explicit offline test provider and never claims hosted reasoning.
- **SQLite:** `rusqlite` in Rust. Startup applies additive `user_version` migrations. A database with an older schema is backed up to a uniquely named `.pre-m1.2.bak.sqlite3` before migration. Investigation JSON is persisted alongside queryable status and metadata columns.
- **Frontend:** same-origin browser workspace served by Axum and driven by actual investigation/history/settings API responses.

## Investigation flow

1. Rust writes a `planning` investigation record.
2. A structured planner call selects up to five specialist subproblems. Invalid schemas receive at most one repair request.
3. Rust calls each selected specialist separately and validates tool names against that specialist's allowlist.
4. Approved tool arguments are passed to Python using the versioned worker contract. Tool outputs and methods are saved as calculated results; numerical output is not experimental validation.
5. Rust downgrades claims without matching retrieved source provenance or calculation references. No scholarly retrieval is configured in this milestone, so unsupported model claims remain hypotheses.
6. A separate synthesis call produces a concise report. SQLite persists intermediate and final workflow state.

Execution is bounded by a per-request provider timeout, at most five specialist roles, a logical-call/token budget, bounded HTTP retries, and a server request timeout. Model prompts ask for concise conclusions and do not request hidden chain-of-thought. No generated command or code is executed.

## SQLite choice

`rusqlite` is a maintained, direct SQLite binding with bundled SQLite and online backup support. It keeps persistence local to the Rust application, avoids a separate database service, and provides a backup API before schema changes. Python's former database schema (`id`, `question`, `created_at`, `result`) is extended additively so legacy records remain queryable.

## Current limitations

- Hosted inference has not been tested: the local environment has no configured model key. Provider transport, retries, usage parsing, authentication and timeout paths are tested with injected fake transport.
- SQLite fallback is available. Zep, scholarly retrieval, Lean, document ingestion, and 3D visualization belong to later milestones.
- Python's current tools use the standard library (arithmetic, regression fit, polynomial evaluation, descriptive statistics, projectile motion). NumPy, SciPy, SymPy, pandas and plotting packages could not be installed in this environment: the existing virtual environment is MSYS2/UCRT Python 3.14, for which the configured package index supplied no compatible prebuilt wheels. No native source build was attempted.
- Specialist execution is bounded and separate per selected role but currently serial. Concurrency scheduling, evidence retrieval, and independent cross-specialist contradiction modeling remain future work.
- No browser-to-server smoke test is claimed until loopback access is verified in this environment.
