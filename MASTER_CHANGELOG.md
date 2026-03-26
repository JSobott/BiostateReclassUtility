# MASTER_CHANGELOG

## 2026-03-26 — CCR #1: Comprehensive Creation Review

Full-codebase quality sweep across all 5 functional areas (QBO API, Data Ingestion, Database, Server, Frontend). 7 Critical and 19 High findings identified; all Critical and code-level High issues fixed.

### Critical Fixes
- **OAuth CSRF bypass**: State check skipped on server restart; nonce never consumed (replay). Fixed with `take()` + None rejection.
- **Retry logic**: `tokio_retry` retried fatal 400/401 errors. Replaced with manual loop (only 429/5xx retry).
- **Writeback panics**: `unwrap()` on untrusted QBO JSON. Replaced with error handling.
- **Silent class-list failure**: Empty class map caused all writebacks to fail with wrong error. Now aborts batch.
- **Token refresh cascade**: 401 during concurrent fetch silently dropped all remaining transactions. Added abort flag + error return.

### High Fixes
- **DB hardening**: Added WAL mode, busy_timeout, foreign_keys PRAGMAs.
- **DB Mutex poison**: Now logs error + returns SQLITE_BUSY instead of misleading QueryReturnedNoRows.
- **Status transition guards**: `update_status` now enforces forward-only transitions.
- **Silent error swallowing**: All 12 `let _ =` on DB writes replaced with `if let Err` + error logging.
- **SSE connection leak**: Stream now terminates on done/error.
- **LLM timeout**: Added 120s request + 10s connect timeout via static client.
- **LLM connection pooling**: Replaced per-call `Client::new()` with `LazyLock` singleton.
- **LLM prompt quality**: Debug `{:?}` formatting replaced with clean `N/A` formatting.
- **Classes endpoint**: Returns 503 on error instead of misleading empty 200.

### Files Modified
- `src/server.rs` — OAuth, writeback, SSE, error handling (1006→1059 lines)
- `src/db.rs` — PRAGMAs, lock_conn, update_status guards (491→522 lines)
- `src/qbo_client.rs` — Manual retry loop (345→348 lines)
- `src/llm_engine.rs` — Static client, timeout, prompt formatting (117→129 lines)
- `src/sync.rs` — Abort flag, error escalation (551→577 lines)

---

## 2026-03-26 — Initial Documentation

- Created `COMPENDIUM.md` (project specs, human-readable overview)
- Created `PROJECT_NOTES.md` (code structure and module guide)
- Created `MASTER_CHANGELOG.md` (this file)

### Codebase State at Time of Documentation

- **Version**: 0.1.0
- **Commits**: 4 (Initial commit through docs/GitHub Pages setup)
- **Rust source**: ~2,794 lines across 8 modules
- **Status**: Core pipeline implemented (ingestion, LLM classification, HITL review UI, QBO writeback with audit logging). OAuth flow, Keychain secrets, and classification rules all functional.
