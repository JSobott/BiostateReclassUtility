# ReclassUtility: Agent Instructions
<!-- Purpose: Provide context, constraints, and operational guidelines for any AI agent working on the ReclassUtility codebase. Owner Agent Version: 1.0 -->

## 1. Project Context
The **ReclassUtility** is a Rust-based tool designed to automate the assignment of classes to unclassified transactions in QuickBooks Online (QBO). It fetches unclassified transactions, runs them through a local LLM to predict the correct class based on entity name, memo, account, and purchase order, and presents these predictions to a human accountant for approval via a web UI. Once approved, the transactions are updated in QBO.

## 2. Tech Stack Setup
- **Language**: Rust
- **Web Framework**: `axum` (with a simple HTML/JS frontend for the Human-In-The-Loop review).
- **QBO API Client**: `reqwest` for HTTP requests.
- **LLM Integration**: `async-openai` (configured to point to a local proxy like Ollama or Llama.cpp operating on localhost).
- **Database / State**: `rusqlite` for local state persistence and caching of transactions.
- **Secrets Management**: macOS Keychain (via the `security-framework` crate or similar macOS-specific credential managers) for storing QBO OAuth tokens and Client IDs. Do NOT use `.env` files for secrets.

## 3. Core Architectural Rules

### Data Ingestion
- Retrieve transactions within a given date range.
- Filter strictly for transactions where `ClassRef` is null or empty.

### LLM Constraints
- The LLM inference runs **locally** taking advantage of Apple Silicon (M4 Mac Mini with 24GB Unified Memory). Data privacy is paramount—never make external calls to third-party endpoints (e.g., OpenAI, Anthropic).
- Expect the LLM to return JSON containing the suggested `ClassRef`, reasoning, and a confidence score.

### Human-in-the-Loop (HITL)
- **Zero-Trust Execution**: No transaction is written back to QBO without explicit human approval.
- The Axum frontend will serve as the dashboard for accountants to batch-review, approve, modify, or reject the LLM's proposals.

### QBO API Write-Back (CRITICAL GUARDRAILS)
- **Sparse Updates ONLY**: Payload submissions to the QBO API must be sparse updates. The agent is **ONLY allowed to edit the class (`ClassRef`)**. No other field (amount, date, account, memo, etc.) can be updated under any circumstances.
- **Idempotency**: Before any API write, the application must verify the item's `SyncToken` to prevent accidental duplication or data loss.
- Provide robust error handling for API write limitations and rate limits.

## 4. Development & Execution Protocol
- **Zero-Trust Execution**: Never assume an API call or database write succeeded; verify the state change.
- **Traceability**: Wrap IO operations and API calls in robust error handling with descriptive logging (`tracing` crate recommended).
- **Modularity**: Keep functions focused. Separate API communication, LLM inference, database operations, and HTTP routing into distinct modules.
- **Persistence**: Use `rusqlite` to store state. If the app crashes, it should be able to resume pending HITL approvals without requiring re-ingestion or re-inferencing.
