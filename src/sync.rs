// Purpose: Live QBO data ingestion and LLM-inference pipeline (via General Ledger report).
// Owner: Antigravity Agent
use crate::db::AppDb;
use crate::llm_engine;
use crate::models::{ClassificationRule, TransactionLine, TransactionStatus};
use crate::qbo_client::QboClient;
use crate::secrets;
use chrono::{Duration, Local};
use serde_json::Value;
use std::collections::HashSet;

use chrono::NaiveDate;
use futures::stream::{self, StreamExt};
use std::sync::Arc;

pub async fn run_sync_job(
    db: AppDb,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let s_date = start_date.unwrap_or_else(|| {
        (Local::now() - Duration::days(30))
            .format("%Y-%m-%d")
            .to_string()
    });
    let e_date = end_date.unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());

    let start_dt = NaiveDate::parse_from_str(&s_date, "%Y-%m-%d")?;
    let end_dt = NaiveDate::parse_from_str(&e_date, "%Y-%m-%d")?;

    // 1. Get Tokens from Keychain
    let (access_token, refresh_token, realm_id) = match secrets::get_oauth_data() {
        Ok(data) => data,
        Err(e) => {
            tracing::error!(
                "Missing OAuth tokens in Keychain. Run the server and login at /auth/login first."
            );
            return Err(e);
        }
    };

    let mut qbo_client = QboClient::new(realm_id.clone(), access_token);

    // 2. Fetch the General Ledger report from QBO in 30-day chunks
    tracing::info!("Fetching QBO General Ledger for {} to {}", s_date, e_date);

    let mut chunks = Vec::new();
    let mut current_start = start_dt;
    while current_start <= end_dt {
        let chunk_end = current_start + Duration::days(29);
        let current_end = if chunk_end > end_dt {
            end_dt
        } else {
            chunk_end
        };
        chunks.push((
            current_start.format("%Y-%m-%d").to_string(),
            current_end.format("%Y-%m-%d").to_string(),
        ));
        current_start = current_end + Duration::days(1);
    }

    let mut gl_entries: Vec<GlEntry> = Vec::new();
    let mut seen_tx: HashSet<(String, String)> = HashSet::new();

    for (chunk_start, chunk_end) in chunks {
        tracing::info!("  -> Fetching GL chunk: {} to {}", chunk_start, chunk_end);
        let report = match qbo_client
            .fetch_general_ledger(&chunk_start, &chunk_end)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                if e.to_string().contains("401") || e.to_string().contains("AuthenticationFailed") {
                    tracing::warn!("QBO Token expired. Refreshing...");
                    let creds = secrets::get_client_credentials()?;
                    let new_tokens =
                        QboClient::refresh_oauth_token(&creds.0, &creds.1, &refresh_token).await?;
                    secrets::store_oauth_data(
                        &new_tokens.access_token,
                        &new_tokens.refresh_token,
                        &realm_id,
                    )?;
                    qbo_client.update_access_token(new_tokens.access_token);
                    tracing::info!("Token refreshed. Retrying GL chunk...");
                    qbo_client
                        .fetch_general_ledger(&chunk_start, &chunk_end)
                        .await?
                } else {
                    return Err(e);
                }
            }
        };

        let mut chunk_entries = Vec::new();
        extract_gl_entries(&report, "Unknown", &mut chunk_entries, &mut seen_tx);
        gl_entries.extend(chunk_entries);
    }

    tracing::info!(
        "Found {} unique classifiable transactions in the GL",
        gl_entries.len()
    );

    // 2b. Fetch active QBO Classes for the LLM prompt
    let available_classes = match qbo_client.fetch_active_classes().await {
        Ok(classes) => {
            tracing::info!("Fetched {} active classes from QBO", classes.len());
            classes
        }
        Err(e) => {
            tracing::warn!(
                "Failed to fetch QBO classes: {}. LLM will use empty list.",
                e
            );
            Vec::new()
        }
    };

    // 2c. Load classification rules from DB
    let rules = db.get_rules().unwrap_or_else(|e| {
        tracing::warn!("Failed to load classification rules from DB: {}. Proceeding without rules.", e);
        Vec::new()
    });
    if !rules.is_empty() {
        tracing::info!("Loaded {} classification rules", rules.len());
    }

    // 4. Fetch the full entity to inspect its Line items concurrently.
    //    Use an abort flag to detect 401 cascades and stop wasting API calls.
    let qbo_client_arc = Arc::new(qbo_client);
    let abort_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let concurrent_fetch_limit = 3; // Strict limit to avoid Intuit 429 ThrottleExceeded

    tracing::info!(
        "Fetching detailed entity data for {} items (concurrency={})...",
        gl_entries.len(),
        concurrent_fetch_limit
    );

    let fetch_results: Vec<Result<Vec<TransactionLine>, Box<dyn std::error::Error>>> = stream::iter(gl_entries.into_iter())
        .map(|entry| {
            let qbo = Arc::clone(&qbo_client_arc);
            let abort = Arc::clone(&abort_flag);
            async move {
                // Check abort flag before starting — if token expired, skip remaining fetches
                if abort.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(format!("Skipped {} id={}: abort flag set due to earlier auth failure", entry.api_type, entry.tx_id).into());
                }

                // Add an artificial delay to prevent burst limit triggers per Intuit docs
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                match qbo.fetch_transaction_by_id(&entry.api_type, &entry.tx_id).await {
                    Ok(entity) => {
                        let mut lines = Vec::new();
                        let (_, sync_token, entity_name, header_memo) = extract_header_info(&entity);
                        let tx_date = entity.get("TxnDate").and_then(|d| d.as_str()).map(|s| s.to_string()).or_else(|| entry.gl_date.clone());

                        if let Some(entity_lines) = entity.get("Line").and_then(|l| l.as_array()) {
                            for line in entity_lines {
                                if !is_missing_class(line) { continue; }
                                if line.get("DetailType").and_then(|t| t.as_str()) == Some("DescriptionOnly") { continue; }

                                let line_id = line["Id"].as_str().unwrap_or("unknown").to_string();

                                // Prevent duplicates
                                if lines.iter().any(|l: &TransactionLine| l.tx_id == entry.tx_id && l.line_id == line_id) { continue; }

                                let account_type = entry.gl_account_type.clone();

                                lines.push(TransactionLine {
                                    tx_id: entry.tx_id.clone(),
                                    line_id,
                                    sync_token: sync_token.clone(),
                                    tx_type: entry.api_type.clone(),
                                    tx_date: tx_date.clone(),
                                    entity_name: entity_name.clone(),
                                    header_memo: header_memo.clone().or_else(|| entry.gl_memo.clone()),
                                    line_description: line.get("Description").and_then(|d| d.as_str()).map(|d| d.to_string()),
                                    account: extract_account_name(line).or_else(|| Some(entry.gl_account.clone())),
                                    account_type,
                                    po_ref: None,
                                    amount: line.get("Amount").and_then(|a| a.as_f64()),
                                    suggested_class_ref: None,
                                    llm_reasoning: None,
                                    confidence_score: None,
                                    status: TransactionStatus::Pending,
                                });
                            }
                        }
                        Ok(lines)
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        if err_str.contains("401") || err_str.contains("AuthenticationFailed") {
                            tracing::error!("Token expired during concurrent fetch for {} id={}. Setting abort flag to skip remaining fetches.", entry.api_type, entry.tx_id);
                            abort.store(true, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            tracing::warn!("Failed to fetch {} id={}: {}", entry.api_type, entry.tx_id, e);
                        }
                        Err(e)
                    }
                }
            }
        })
        .buffer_unordered(concurrent_fetch_limit)
        .collect::<Vec<_>>().await;

    let total_attempted = fetch_results.len();
    let mut fetch_failures = 0;
    let mut auth_aborted = false;
    let unclassified_nested: Vec<Vec<TransactionLine>> = fetch_results
        .into_iter()
        .filter_map(|res| match res {
            Ok(lines) => Some(lines),
            Err(e) => {
                fetch_failures += 1;
                if e.to_string().contains("abort flag") || e.to_string().contains("AuthenticationFailed") {
                    auth_aborted = true;
                }
                None
            }
        })
        .collect();

    if fetch_failures > 0 {
        tracing::error!(
            "{}/{} entity fetches failed. These transactions will NOT appear in the review UI.{}",
            fetch_failures,
            total_attempted,
            if auth_aborted { " CAUSE: OAuth token expired mid-sync. Re-run sync after refreshing tokens." } else { "" }
        );
    }

    // If auth failure caused a cascade, return an error so the user knows the sync was incomplete
    if auth_aborted && fetch_failures > total_attempted / 2 {
        return Err(format!(
            "Sync aborted: OAuth token expired mid-sync. {}/{} entity fetches failed. Please refresh your token and re-run the sync.",
            fetch_failures, total_attempted
        ).into());
    }

    let mut unclassified_lines: Vec<TransactionLine> =
        unclassified_nested.into_iter().flatten().collect();

    tracing::info!(
        "Extracted {} unclassified lines total.",
        unclassified_lines.len()
    );

    // 5. Apply heuristic rules first (before LLM).
    let mut rule_applied_count = 0;
    for line in &mut unclassified_lines {
        if let Some(target_class) = find_matching_rule(line, &rules) {
            line.suggested_class_ref = Some(target_class.clone());
            line.llm_reasoning = Some("Applied heuristic rule".to_string());
            line.confidence_score = Some(1.0);
            rule_applied_count += 1;
        }
    }
    if rule_applied_count > 0 {
        tracing::info!(
            "{} lines classified by heuristic rules (skipping LLM)",
            rule_applied_count
        );
    }

    // 6. Persist ALL lines immediately so they are visible in the review UI.
    for line_item in &unclassified_lines {
        if let Err(e) = db.insert_transaction(line_item) {
            tracing::error!(
                "DB insert failed [{} {}]: {}",
                line_item.tx_id,
                line_item.line_id,
                e
            );
        }
    }
    tracing::info!(
        "{} lines persisted (Upserted safely).",
        unclassified_lines.len()
    );

    // 7. Send only non-rule-matched lines to Phi-3 Mini concurrently.
    let unclassified_to_infer: Vec<TransactionLine> = unclassified_lines
        .into_iter()
        .filter(|l| {
            !(l.confidence_score == Some(1.0)
                && l.llm_reasoning.as_deref() == Some("Applied heuristic rule"))
        })
        .collect();

    if !unclassified_to_infer.is_empty() {
        tracing::info!(
            "Starting concurrent LLM inference for {} items...",
            unclassified_to_infer.len()
        );

        let classes_arc = Arc::new(available_classes);
        let db_arc = Arc::new(db);
        let concurrent_llm_limit = 5; // Phi-3 Mini concurrency limit

        stream::iter(unclassified_to_infer)
            .map(|line| {
                let classes = Arc::clone(&classes_arc);
                let db_local = Arc::clone(&db_arc);
                async move {
                    tracing::debug!("Inferencing [{} {}]...", line.tx_id, line.line_id);
                    match llm_engine::predict_class(&line, &classes).await {
                        Ok(prediction) => {
                            if let Err(e) = db_local.update_inference(
                                &line.tx_id,
                                &line.line_id,
                                &prediction.class_ref,
                                &prediction.reasoning,
                                prediction.confidence_score,
                            ) {
                                tracing::error!(
                                    "Failed to update inference [{} {}]: {}",
                                    line.tx_id,
                                    line.line_id,
                                    e
                                );
                            } else {
                                tracing::info!(
                                    "=> [{} {}] class='{}' ({:.0}%)",
                                    line.tx_id,
                                    line.line_id,
                                    prediction.class_ref,
                                    prediction.confidence_score * 100.0
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "LLM failed for [{} {}]: {}",
                                line.tx_id,
                                line.line_id,
                                e
                            );
                        }
                    }
                }
            })
            .buffer_unordered(concurrent_llm_limit)
            .collect::<Vec<()>>()
            .await;
    }

    tracing::info!("Sync complete. Run without --sync to start the review server.");
    Ok(())
}

/// Intermediate struct for GL posting data before entity fetch
struct GlEntry {
    tx_id: String,
    api_type: String,
    gl_account: String,
    gl_account_type: Option<String>,
    gl_date: Option<String>,
    _gl_amount: Option<f64>,
    gl_memo: Option<String>,
}

/// Recursively extracts transaction entries from QBO report rows, handling arbitrary nesting (sub-accounts).
fn extract_gl_entries(
    node: &Value,
    current_account: &str,
    entries: &mut Vec<GlEntry>,
    seen: &mut HashSet<(String, String)>,
) {
    // 1. Check for account header in this section
    let header_name = node
        .get("Header")
        .and_then(|h| h.get("ColData"))
        .and_then(|cd| cd.get(0))
        .and_then(|c| c.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or(current_account);

    // 2. Identify transaction rows (they have ColData)
    if let Some(col_data) = node.get("ColData").and_then(|c| c.as_array()) {
        let raw_tx_type = col_data
            .get(1)
            .and_then(|c| c.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let tx_id = col_data
            .get(1)
            .and_then(|c| c.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !tx_id.is_empty()
            && !raw_tx_type.is_empty()
            && let Some(api_type) = map_report_type_to_api_entity(raw_tx_type)
        {
            let key = (api_type.to_string(), tx_id.to_string());
            if !seen.contains(&key) {
                seen.insert(key);

                let gl_date = col_data
                    .first()
                    .and_then(|c| c.get("value"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let gl_acc_name = col_data
                    .get(3)
                    .and_then(|c| c.get("value"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let gl_acc_type = col_data
                    .get(4)
                    .and_then(|c| c.get("value"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let gl_amount = col_data
                    .get(6)
                    .and_then(|c| c.get("value"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok());
                let gl_memo = col_data
                    .get(2)
                    .and_then(|c| c.get("value"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                entries.push(GlEntry {
                    tx_id: tx_id.to_string(),
                    api_type: api_type.to_string(),
                    gl_account: gl_acc_name.unwrap_or_else(|| header_name.to_string()),
                    gl_account_type: gl_acc_type,
                    gl_date,
                    _gl_amount: gl_amount,
                    gl_memo,
                });
            }
        }
    }

    // 3. Recurse into children
    if let Some(rows) = node
        .get("Rows")
        .and_then(|r| r.get("Row"))
        .and_then(|r| r.as_array())
    {
        for row in rows {
            extract_gl_entries(row, header_name, entries, seen);
        }
    }
}

/// Check if a classification rule matches a transaction line.
fn find_matching_rule(line: &TransactionLine, rules: &[ClassificationRule]) -> Option<String> {
    for rule in rules {
        let field_value = match rule.condition_field.as_str() {
            "account_type" => line.account_type.as_deref(),
            "account" => line.account.as_deref(),
            "tx_type" => Some(line.tx_type.as_str()),
            "entity_name" => line.entity_name.as_deref(),
            _ => None,
        };

        if let Some(val) = field_value
            && val.eq_ignore_ascii_case(&rule.condition_value)
        {
            tracing::info!(
                "Rule match: {} = '{}' → class '{}'",
                rule.condition_field,
                val,
                rule.target_class
            );
            return Some(rule.target_class.clone());
        }
    }
    None
}

fn extract_header_info(tx: &Value) -> (String, String, Option<String>, Option<String>) {
    let id = tx["Id"].as_str().unwrap_or("unknown").to_string();
    let sync = tx["SyncToken"].as_str().unwrap_or("0").to_string();
    let entity = tx
        .get("EntityRef")
        .or_else(|| tx.get("VendorRef"))
        .or_else(|| tx.get("CustomerRef"))
        .and_then(|e| e.get("name"))
        .and_then(|n| n.as_str())
        .map(|n| n.to_string());
    let memo = tx
        .get("PrivateNote")
        .and_then(|m| m.as_str())
        .map(|m| m.to_string());
    (id, sync, entity, memo)
}

/// Returns true when a line item has no ClassRef assigned.
fn is_missing_class(line: &Value) -> bool {
    let detail = line
        .get("AccountBasedExpenseLineDetail")
        .or_else(|| line.get("ItemBasedExpenseLineDetail"))
        .or_else(|| line.get("JournalEntryLineDetail"))
        .or_else(|| line.get("SalesItemLineDetail"));

    match detail {
        Some(d) => d.get("ClassRef").is_none(),
        None => false,
    }
}

fn extract_account_name(line: &Value) -> Option<String> {
    // Support Account-based lines
    if let Some(name) = line
        .get("AccountBasedExpenseLineDetail")
        .and_then(|d| d.get("AccountRef"))
        .and_then(|r| r.get("name"))
        .and_then(|n| n.as_str())
    {
        return Some(name.to_string());
    }

    // Support Item-based lines (Lumber, Materials, etc.)
    if let Some(name) = line
        .get("ItemBasedExpenseLineDetail")
        .and_then(|d| d.get("ItemRef"))
        .and_then(|r| r.get("name"))
        .and_then(|n| n.as_str())
    {
        return Some(format!("Item: {}", name));
    }

    // Support Sales-based lines (Invoices, Sales Receipts)
    if let Some(name) = line
        .get("SalesItemLineDetail")
        .and_then(|d| d.get("ItemRef"))
        .and_then(|r| r.get("name"))
        .and_then(|n| n.as_str())
    {
        return Some(format!("Item: {}", name));
    }

    // Support Journal Entry lines
    if let Some(name) = line
        .get("JournalEntryLineDetail")
        .and_then(|d| d.get("AccountRef"))
        .and_then(|r| r.get("name"))
        .and_then(|n| n.as_str())
    {
        return Some(name.to_string());
    }

    None
}

/// Maps GL report transaction type labels to QBO v3 REST API endpoint names.
fn map_report_type_to_api_entity(report_type: &str) -> Option<&'static str> {
    match report_type {
        "Expense"
        | "Check"
        | "Cash Expense"
        | "Cash Purchase"
        | "Credit Card Expense"
        | "Credit Card Charge" => Some("purchase"),
        "Bill" => Some("bill"),
        "Invoice" => Some("invoice"),
        "Sales Receipt" => Some("salesreceipt"),
        "Journal Entry" => Some("journalentry"),
        "Deposit" => Some("deposit"),
        "Vendor Credit" | "Credit Card Credit" => Some("vendorcredit"),
        _ => None,
    }
}
