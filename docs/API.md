# API contract v1.0

The API is same-origin JSON over HTTP. No cross-origin origins are enabled.

| Method | Path | Request | Success |
| --- | --- | --- | --- |
| `GET` | `/` | — | Research workspace HTML |
| `GET` | `/health`, `/api/health` | — | `{status, contract_version, mode}` |
| `POST` | `/api/investigate` | `{ "question": string }` | Structured investigation |
| `POST` | `/api/history` | `{ "q"?: string }` | `{ "items": [...] }` |
| `POST` | `/api/tool` | `{ "name": string, "args": object }` | `{ "result": ... }` |
| `POST` | `/api/settings` | `{}` | Nonsecret provider and memory settings |

The question must contain 8–4000 characters. Bodies are limited by `MAX_REQUEST_BYTES` (default 262144). Each Python worker invocation is bounded by `WORKER_TIMEOUT_SECONDS` (default 30); the HTTP request has a 60-second cap. Axum handles HTTP framing and Content-Length. Unsupported methods receive HTTP 405; unknown paths receive 404.

Error bodies use `{ "error": string, "code": string }`. Malformed JSON and invalid requests return 400, oversized requests 413, unavailable workers 503, worker timeouts 504, hosted-provider failures 502, and unexpected worker failures 500. The API validates typed request structures in Rust; deeper scientific argument validation is currently performed by the Python tools.
