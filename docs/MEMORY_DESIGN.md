# Memory design

SQLite remains the reliable local history store. The existing `data/neurogenesis.sqlite3` and its `investigations` schema were inspected and preserved; no migration or cleanup was performed. Investigations store ID, question, timestamp, and structured result, and search filters persisted questions. The database is ignored by Git.

The current Python application still owns database reads and writes. The next architecture milestone should place a Rust memory trait and SQLite implementation behind the API; rusqlite was not present in the available offline Cargo cache during Milestone 1.1.

Zep is deliberately deferred until the core workflow is stable. Future integration must use current Zep v4 documentation, the `zep-cloud` package, `ZEP_API_KEY`, and v4 user/thread/context identifiers. Preserve Zep UUIDs locally. Do not assume older endpoints or authentication formats. No Zep credentials are required for SQLite mode.
