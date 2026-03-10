# Product Requirements Document: QBO Reclass Utility

## 1. Executive Summary
The QBO Reclass Utility is a targeted, Rust-based agent built to automate the tedious process of assigning classes to unclassified transactions in QuickBooks Online (QBO). By pulling all unclassified transactions for a specified date range and passing them to an accurate, locally-hosted Large Language Model (LLM), the app proposes the correct class based on entity name, memo, account, and purchase order metadata. A strict Human-in-the-Loop (HITL) batch approval step ensures that all proposals are reviewed by an accountant before being securely posted back to the QBO ledger via the API.

## 2. Target Persona
- **Controllers / Accountants**: Financial professionals seeking to expedite month-end close by automating the classification of bulk uncategorized transactions, without compromising on data privacy or accuracy.

## 3. Product Goals
- **Automated Classification**: Automatically map raw transaction data to the company's established class structure.
- **Maximum Data Privacy**: Execute all LLM inference strictly locally on an Apple Silicon M4 Mac Mini, ensuring sensitive financial data is never transmitted to closed-source third parties (e.g., OpenAI, Anthropic).
- **High Accuracy over Speed**: Prioritize models with superior instruction-following and reasoning capabilities. Slower inference times are acceptable.
- **Zero-Trust Execution**: No transaction is written back to QBO without explicit human approval (HITL).

## 4. Functional Requirements

### 4.1. Data Ingestion (QBO API)
- Authenticate with the QuickBooks Online API using OAuth 2.0.
- Retrieve all transactions (Expense, Journal Entry, Deposit, etc.) within a user-defined date range.
- Filter results to isolate line items where the `ClassRef` is null or empty.
- Extract the necessary contextual data points for each line item: `Entity Name` (Vendor/Customer), `Memo`, `Account`, and `Purchase Order` reference.

### 4.2. Local AI Classification Engine
- Transmit the extracted transaction metadata to a local LLM environment (e.g., `Ollama`, or `Llama.cpp` for native Metal support).
- Utilize a highly capable, instruction-following model that comfortably fits in 24GB of unified memory (e.g., Llama-3.1 8B Instruct, Mistral Nemo 12B, or a quantized Qwen 2.5 14B). 
- Prompting rules must enforce a structured output (e.g., JSON schema) containing:
  - The suggested `ClassRef` (mapped to the target company's existing Classes).
  - A brief rationale/reasoning trail for the classification.
  - A predicted confidence score.

### 4.3. Human-in-the-Loop (HITL) Interface
- Aggregate LLM predictions into digestible review batches.
- Provide a responsive UI (either a Terminal UI via `ratatui` or a lightweight local web dashboard) that presents the transaction context alongside the LLM's suggested class and reasoning.
- Enable the user to dynamically "Approve", "Modify", or "Reject" proposals.

### 4.4. QBO API Write-Back
- Collect the human-approved transaction classes.
- Submit sparse update payloads back to the QBO API to properly reclassify the transactions on the remote ledger. This must be a sparse update that is only allowed to edit the class (`ClassRef`) - NO OTHER FIELD CAN BE UPDATED.
- Implement robust error handling for API write limitations, rate limits, and sync token mismatch issues.

## 5. Technical Architecture

- **Language**: Rust (Chosen for its type safety, low-overhead concurrency, and reliable memory management).
- **Networking**: `reqwest` for executing QBO REST API calls. 
- **LLM Connectivity**: `async-openai` library configured to point to the local server (e.g., `http://localhost:11434/v1` for Ollama).
- **State Management / Idempotency**: `rusqlite` (SQLite) to locally cache transactions during processing. If the application crashes before the HITL write-back, the local state can be resumed without re-querying QBO or re-inferencing the LLM.
- **HITL UX**: `axum` + simple HTML/JS frontend to present the transaction context alongside the LLM's suggested class and reasoning.

## 6. Specific Constraints & Guardrails
- **Hardware Optimization**: The system must be configured to utilize Apple's MLX or CoreML/Metal optimizations to fully harness the M4 Mac Mini architecture.
- **Idempotency**: Retrying a failed QBO update must verify the item's SyncToken to prevent accidental duplication or mutation loss. 
- **Secrets Management**: Ensure OAuth tokens and QBO Client IDs/Secrets are securely handled in the macOS Keychain and excluded from version control.

## 7. Success Criteria
1. The tool successfully pulls 100% of unclassified transactions for a given period.
2. The local LLM maps transactions accurately >90% of the time, properly adhering to the provided Class JSON schema list.
3. The HITL review process easily visualizes all metadata and allows seamless user overrides.
4. The QBO write-back applies the Approved classes without corrupting existing transaction data.
