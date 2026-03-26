# ReclassUtility - Project Compendium

## What Is This?

ReclassUtility is a desktop application that automates the classification of unclassified transactions in QuickBooks Online (QBO). It pulls transactions missing a class assignment, runs them through a locally-hosted AI model to predict the correct class, and presents the results to an accountant for review. Only after human approval are the classifications written back to QBO.

The entire pipeline runs on the user's local machine. No financial data ever leaves the device.

## Who Is It For?

Controllers and accountants who need to classify large volumes of unclassified QBO transactions during month-end close. The tool turns hours of manual classification work into a review-and-approve workflow.

## How It Works

### 1. Data Ingestion

The app authenticates with QBO via OAuth 2.0, then fetches the General Ledger report in 30-day chunks. It parses the report to find all transaction line items where `ClassRef` is null (i.e., unclassified). For each unclassified line, it fetches the full transaction entity (Bill, Purchase, Invoice, JournalEntry, etc.) to gather context: entity name, memo, account, amount, and PO reference.

### 2. AI Classification

Each unclassified line is sent to a local LLM server (MLX-LM, Ollama, or Llama.cpp) running on `localhost:8080`. The model receives the transaction metadata and the company's class list, then returns a structured JSON response containing a suggested class, reasoning, and confidence score. Up to 5 inference requests run concurrently.

Before LLM inference, user-defined heuristic rules are checked first (e.g., "if account_type is Accounts Payable, assign class Balance Sheet"). Lines matching a rule skip the LLM entirely.

### 3. Human Review (HITL)

An Axum-powered web UI at `localhost:3000` presents the results. Transactions are grouped by account, entity name, and suggested class for efficient batch review. The accountant can:

- **Approve** a group (applies the suggested class to all lines in the bucket)
- **Override** (approve with a different class selected from a dropdown)
- **Reject** (exclude from writeback)
- **Drill down** to review individual transactions within a group

### 4. QBO Writeback

Approved transactions are written back to QBO using sparse updates — only the `ClassRef` field is modified, no other fields are touched. Before each update, the app fetches the latest `SyncToken` from QBO to prevent conflicts. Retries with exponential backoff handle rate limits (429) and transient errors (5xx). All requests and responses are logged to an audit table.

Progress is streamed to the UI via Server-Sent Events (SSE).

## Key Design Principles

- **Local-first privacy**: All data stays on the user's machine. LLM inference is local. OAuth tokens are stored in macOS Keychain, not files.
- **Zero-trust execution**: No transaction is written back without explicit human approval.
- **Sparse updates only**: The writeback payload can only modify `ClassRef`. This is a hard constraint — no other transaction fields can be changed.
- **Crash resilience**: SQLite stores all state locally. If the app crashes, work resumes from where it left off without re-fetching or re-classifying.
- **Audit trail**: Every writeback request/response is logged with timestamps and UUIDs.

## Hardware & Runtime Requirements

- **Target hardware**: Apple Silicon Mac (M4 Mac Mini with 24GB unified memory is the reference platform)
- **LLM server**: MLX-LM (`mlx_lm.server`), Ollama, or Llama.cpp running on port 8080
- **Recommended models**: Phi-3 Mini 4K, Llama 3.1 8B Instruct, Qwen 2.5 14B (quantized)
- **QBO account**: QuickBooks Online with Classes enabled and an OAuth app configured

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust (Edition 2024) |
| Web framework | Axum 0.8 |
| Async runtime | Tokio 1.50 |
| Database | SQLite via rusqlite 0.38 (bundled) |
| HTTP client | reqwest 0.13 |
| Credential storage | macOS Keychain (via `security` CLI) |
| Frontend | Vanilla HTML/CSS/JS (single-page) |
| Logging | tracing + tracing-subscriber |
| CLI | clap 4.5 (derive) |

## Usage

```bash
# Store OAuth credentials in Keychain
cargo run -- --client-id <YOUR_CLIENT_ID> --client-secret <YOUR_SECRET>

# Run a one-off sync (fetch + classify, no server)
cargo run -- --sync [--start-date YYYY-MM-DD] [--end-date YYYY-MM-DD]

# Start the review server
cargo run
# Then open http://localhost:3000
```

## API Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/pending` | List all pending transaction lines |
| GET | `/api/groups` | Grouped view (account, entity, class) |
| GET | `/api/group-items` | Items within a specific group |
| POST | `/api/batch-approve` | Approve an entire group |
| POST | `/api/approve` | Approve a single transaction |
| POST | `/api/reset-pending` | Reset a failed group to Pending |
| GET | `/api/rules` | List classification rules |
| POST | `/api/rules` | Create a classification rule |
| POST | `/api/rules/delete` | Delete a classification rule |
| GET | `/api/classes` | Fetch QBO class list for dropdowns |
| POST | `/api/writeback` | Trigger writeback of approved transactions |
| GET | `/api/writeback-progress` | SSE stream for writeback progress |
| GET | `/api/audit-logs` | View writeback audit trail |
| GET | `/api/diagnostics/status` | Check OAuth connection status |
| GET | `/api/diagnostics/company-info` | Fetch QBO company metadata |
| POST | `/api/diagnostics/refresh` | Manually refresh OAuth token |
| POST | `/api/diagnostics/seed-tokens` | Inject OAuth tokens (testing) |
| GET | `/auth/login` | Initiate OAuth login flow |
| GET | `/auth/disconnect` | Clear stored OAuth tokens |
| GET | `/callback` | OAuth callback handler |

## Database Schema

Three tables in `reclass_utility.db`:

**transaction_lines** — Core transaction data with classification state
- Primary key: `(tx_id, line_id)`
- Status lifecycle: `Pending` -> `Approved` -> `Validated` -> `Posted` (or `Failed` / `Rejected`)

**classification_rules** — User-defined heuristic rules that bypass LLM inference
- Conditions: `account_type`, `account`, `tx_type`, `entity_name`

**writeback_audit** — Immutable log of all QBO API write operations
- Stores full request/response JSON for compliance

## Security

- CSRF protection via random 32-byte state nonce on OAuth
- Security headers: `Cache-Control`, `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`
- TRACE/CONNECT methods rejected
- OAuth callback always returns 302 (prevents Referer leakage)
- All QBO communication over TLS/HTTPS

## License

MIT License - Copyright (c) 2026 JSobott
