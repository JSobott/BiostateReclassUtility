# PROJECT_NOTES - Code Structure & Module Guide

## Repository Layout

```
BiostateReclassUtility/
├── src/
│   ├── main.rs            (65 lines)   CLI entrypoint
│   ├── server.rs          (1006 lines) Axum HTTP server & all endpoints
│   ├── sync.rs            (551 lines)  Data ingestion pipeline
│   ├── db.rs              (491 lines)  SQLite persistence layer
│   ├── qbo_client.rs      (345 lines)  QBO REST API client
│   ├── secrets.rs         (119 lines)  macOS Keychain integration
│   ├── llm_engine.rs      (117 lines)  Local LLM inference wrapper
│   └── models.rs          (100 lines)  Domain entities & DTOs
├── static/
│   ├── index.html                      Main review dashboard (SPA)
│   ├── eula.html                       EULA page
│   └── privacy.html                    Privacy policy page
├── docs/                               GitHub Pages documentation
├── Cargo.toml                          Dependencies (Rust Edition 2024)
├── Cargo.lock
├── PRD.md                              Product requirements document
├── agent.md                            AI agent development guidelines
├── EULA.md / PRIVACY.md               Legal documents (Markdown)
└── LICENSE                             MIT
```

**Total Rust**: ~2,794 lines across 8 modules.

## Module Responsibilities

### main.rs
CLI entrypoint. Parses args via `clap` (derive). Three modes:
1. `--client-id` + `--client-secret` → store OAuth creds in Keychain, exit
2. `--sync` → run ingestion pipeline (fetch + classify), exit
3. Default → start Axum server on port 3000

Initializes SQLite DB and Tokio runtime.

### server.rs (largest module)
All HTTP routing and request handling. Key sections:
- **AppState**: `Arc<Mutex<Connection>>` + `Arc<QboClient>` shared across handlers
- **OAuth flow**: `/auth/login`, `/callback`, `/auth/disconnect`
- **CRUD endpoints**: pending, groups, group-items, rules, approve, batch-approve
- **Writeback logic**: Fetches latest entity, constructs sparse update, posts to QBO
- **SSE progress**: `/api/writeback-progress` streams real-time writeback status
- **Security middleware**: Headers via `tower-http`, method filtering

### sync.rs
Data ingestion pipeline, orchestrated by `run_sync()`:
1. `fetch_gl_report()` — Queries QBO General Ledger in 30-day date chunks
2. `extract_gl_entries()` — Recursively walks nested GL JSON, extracts tx metadata
3. `fetch_full_entities()` — Concurrently fetches complete transaction objects (3 at a time)
4. `filter_unclassified_lines()` — Extracts lines where ClassRef is null
5. `apply_rules()` — Matches against user-defined classification rules
6. `run_llm_inference()` — Sends remaining lines to local LLM (5 concurrent)
7. Upserts all results to SQLite with status=Pending

### db.rs
SQLite operations via `rusqlite`. Handles:
- Schema migration (`init_db`) — creates tables idempotently
- UPSERT for transaction lines (dedup on `tx_id + line_id`)
- Status transitions (Pending → Approved → Validated → Posted/Failed)
- Classification rules CRUD
- Audit log inserts and queries
- Group queries (aggregation by account/entity/class)

### qbo_client.rs
QBO REST API wrapper. Key methods:
- `fetch_general_ledger()` — GL report with date range params
- `fetch_entity()` — GET individual transaction by type and ID
- `update_entity()` — POST sparse update with SyncToken
- `query_classes()` — Fetch active class list
- `refresh_token()` — Exchange refresh token for new access token
- `get_company_info()` — Fetch CompanyInfo metadata

All calls include Bearer auth, retry on 401 (auto-refresh), and structured error logging.

### llm_engine.rs
Wraps the local LLM server (OpenAI-compatible `/v1/chat/completions` endpoint on port 8080):
- Builds system prompt with company class list
- Sends transaction context as user message
- Parses response JSON (with depth-aware extraction for hallucinated wrappers)
- Returns `(class_ref, reasoning, confidence_score)`
- Temperature: 0.0 (deterministic output)

### secrets.rs
macOS Keychain integration via the `security` CLI command:
- `set_secret(account, value)` — store/update a credential
- `get_secret(account)` — retrieve a credential
- Service name: `"ReclassUtility"`
- Accounts: `qbo_access_token`, `qbo_refresh_token`, `qbo_realm_id`, `qbo_client_id`, `qbo_client_secret`

### models.rs
Domain types and serialization:
- `TransactionLine` — Core entity (maps to `transaction_lines` table)
- `ClassificationRule` — Heuristic rule definition
- `WritebackAudit` — Audit log entry
- `LineStatus` enum — `Pending`, `Approved`, `Rejected`, `Validated`, `Failed`, `Posted`
- Various API request/response DTOs

## Shared State Pattern

```
AppState {
    db: Arc<Mutex<rusqlite::Connection>>,
    qbo: Arc<QboClient>,
}
```

Passed to all Axum handlers via `State(app_state)`. The `Mutex<Connection>` ensures single-writer SQLite access across async tasks.

## Data Flow Summary

```
QBO GL Report
    ↓ (30-day chunks)
Extract GL Entries
    ↓ (recursive JSON walk)
Fetch Full Entities
    ↓ (3 concurrent, 500ms delay)
Filter Unclassified Lines
    ↓
Apply Heuristic Rules  ──→  Direct classification (skip LLM)
    ↓ (unmatched)
Local LLM Inference
    ↓ (5 concurrent)
SQLite (status=Pending)
    ↓
HITL Web UI Review  ──→  Approve / Override / Reject
    ↓ (Approved only)
QBO Sparse Update
    ↓ (SyncToken verified, retry w/ backoff)
Audit Log
```

## Key Conventions

- **Error handling**: `anyhow::Result` for fallible operations, `tracing::warn!`/`error!` for logging failures. No silent swallowing.
- **Concurrency limits**: 3 concurrent QBO fetches, 5 concurrent LLM requests. Hard-coded to respect rate limits.
- **Date chunking**: GL queries split into 30-day windows. 60-second sleep between chunks.
- **Entity delay**: 500ms pause between individual entity fetches to avoid QBO throttling.
- **Status lifecycle**: `Pending → Approved → Validated → Posted` (happy path). `Failed` on writeback error. `Rejected` by user.
- **Sparse updates**: Writeback payloads modify ONLY the `ClassRef` field. This is a non-negotiable constraint.

## Frontend (static/index.html)

Single-page vanilla HTML/CSS/JS dashboard. No build step, no framework. Communicates with the Axum backend via `fetch()` calls. Features:
- Stats bar (pending count, avg confidence)
- Grouped transaction table with expand/collapse
- Class override dropdown (populated from QBO)
- Batch approve/reject buttons
- Writeback trigger with SSE progress bar
- Classification rules management panel
- OAuth diagnostics panel
