// Purpose: Axum web server for the ReclassUtility review UI.
// Owner: Antigravity Agent
use axum::http::{HeaderValue, Method, StatusCode};
use axum::{
    Json, Router,
    extract::{Query, State},
    response::{
        IntoResponse, Redirect,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::Value;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use urlencoding::encode;

use crate::db::AppDb;
use crate::models::{ClassificationRule, TransactionStatus};
use crate::qbo_client::QboClient;
use crate::secrets;

const OAUTH_REDIRECT_URI: &str = "https://developer.intuit.com/v2/OAuth2Playground/RedirectUrl";

#[derive(Clone)]
struct AppState {
    db: AppDb,
    /// CSRF nonce for OAuth state verification
    oauth_state: Arc<std::sync::Mutex<Option<String>>>,
    /// Broadcast channel for Server-Sent Events to notify progress
    progress_tx: Arc<broadcast::Sender<String>>,
}

pub async fn start_server(db: AppDb) -> Result<(), Box<dyn std::error::Error>> {
    let (progress_tx, _rx) = broadcast::channel(100);

    let state = AppState {
        db,
        oauth_state: Arc::new(std::sync::Mutex::new(None)),
        progress_tx: Arc::new(progress_tx),
    };

    let app = Router::new()
        .route("/api/pending", get(get_pending_transactions))
        .route("/api/groups", get(get_grouped_transactions))
        .route("/api/group-items", get(get_group_items))
        .route("/api/batch-approve", post(batch_approve))
        .route("/api/reset-pending", post(reset_pending))
        .route("/api/approve", post(approve_transaction))
        .route("/api/rules", get(get_rules))
        .route("/api/rules", post(create_rule))
        .route("/api/rules/delete", post(delete_rule))
        .route("/api/classes", get(get_available_classes))
        .route("/api/writeback", post(perform_writeback))
        .route("/api/writeback-progress", get(writeback_progress_sse))
        .route("/api/audit-logs", get(get_audit_logs))
        .route("/api/diagnostics/status", get(get_diagnostics_status))
        .route("/api/diagnostics/company-info", get(get_company_info))
        .route("/api/diagnostics/refresh", post(refresh_token_manual))
        .route("/api/diagnostics/seed-tokens", post(seed_tokens_manual))
        .route("/auth/login", get(oauth_login))
        .route("/auth/disconnect", get(oauth_disconnect))
        .route("/callback", get(oauth_callback))
        .fallback_service(ServeDir::new("static"))
        .with_state(state)
        // --- Intuit Security: response headers ---
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        // --- Intuit Security: block TRACE / CONNECT methods ---
        .layer(axum::middleware::from_fn(reject_disallowed_methods));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

/// Intuit Security: reject TRACE, CONNECT, and other disallowed HTTP methods.
async fn reject_disallowed_methods(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    match *req.method() {
        Method::TRACE | Method::CONNECT => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        _ => next.run(req).await.into_response(),
    }
}

// --- Transaction Endpoints ---

async fn get_pending_transactions(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.get_pending_transactions() {
        Ok(txs) => Json(txs).into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch pending transactions: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
struct GroupQuery {
    status: Option<String>,
}

async fn get_grouped_transactions(
    State(state): State<AppState>,
    Query(params): Query<GroupQuery>,
) -> impl IntoResponse {
    let status = params.status.unwrap_or_else(|| "Pending".to_string());
    match state.db.get_grouped_transactions(&status) {
        Ok(groups) => Json(groups).into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch grouped transactions: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
struct BatchApproveRequest {
    account: String,
    entity_name: String,
    current_class: String,
    override_class: Option<String>,
}

async fn batch_approve(
    State(state): State<AppState>,
    Json(payload): Json<BatchApproveRequest>,
) -> impl IntoResponse {
    let override_ref = payload.override_class.as_deref();

    match state.db.batch_approve_group(
        &payload.account,
        &payload.entity_name,
        &payload.current_class,
        override_ref,
    ) {
        Ok(count) => {
            let final_class = override_ref.unwrap_or(&payload.current_class);
            tracing::info!(
                "Batch approved {} items: entity='{}' account='{}' -> class='{}'",
                count,
                payload.entity_name,
                payload.account,
                final_class
            );
            Json(serde_json::json!({ "status": "success", "count": count })).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to batch approve: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
struct ResetPendingRequest {
    account: String,
    entity_name: String,
    current_class: String,
}

async fn reset_pending(
    State(state): State<AppState>,
    Json(payload): Json<ResetPendingRequest>,
) -> impl IntoResponse {
    match state.db.reset_failed_group_to_pending(
        &payload.account,
        &payload.entity_name,
        &payload.current_class,
    ) {
        Ok(count) => {
            tracing::info!(
                "Reset {} failed items to Pending: entity='{}' account='{}'",
                count,
                payload.entity_name,
                payload.account
            );
            Json(serde_json::json!({ "status": "success", "count": count })).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to reset group to pending: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
struct GroupItemsQuery {
    status: Option<String>,
    account: String,
    entity_name: String,
    suggested_class: String,
}

async fn get_group_items(
    State(state): State<AppState>,
    Query(params): Query<GroupItemsQuery>,
) -> impl IntoResponse {
    let status = params.status.unwrap_or_else(|| "Pending".to_string());
    match state.db.get_group_items(
        &status,
        &params.account,
        &params.entity_name,
        &params.suggested_class,
    ) {
        Ok(items) => Json(items).into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch group items: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
struct ApprovalRequest {
    tx_id: String,
    line_id: String,
    suggested_class_ref: String,
    #[serde(rename = "tx_type")]
    _tx_type: String,
    #[serde(rename = "sync_token")]
    _sync_token: String,
}

async fn approve_transaction(
    State(state): State<AppState>,
    Json(payload): Json<ApprovalRequest>,
) -> impl IntoResponse {
    if let Err(e) = state.db.update_status(
        &payload.tx_id,
        &payload.line_id,
        TransactionStatus::Approved,
    ) {
        tracing::error!("Failed to update status to Approved: {}", e);
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    tracing::info!(
        "TransactionLine {}:{} Approved with class '{}'.",
        payload.tx_id,
        payload.line_id,
        payload.suggested_class_ref
    );
    // Note: Status remains 'Approved' until QBO write-back is implemented,
    // at which point it will transition to 'Posted' after successful API confirmation.

    axum::http::StatusCode::OK.into_response()
}

// --- Write-back Endpoint ---

async fn writeback_progress_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.progress_tx.subscribe();

    let stream = async_stream::stream! {
        while let Ok(msg) = rx.recv().await {
            let is_done = msg.contains("\"status\":\"done\"") || msg.contains("\"status\":\"error\"");
            yield Ok(Event::default().data(msg));
            if is_done {
                break; // Terminate SSE stream after completion/error to free the connection
            }
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new())
}

#[derive(Deserialize)]
struct WritebackRequest {
    dry_run: bool,
}

async fn perform_writeback(
    State(state): State<AppState>,
    Json(payload): Json<WritebackRequest>,
) -> impl IntoResponse {
    let qbo_client = match get_valid_qbo_client().await {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("Failed to obtain valid QBO client: {}", e);
            let _ = state.progress_tx.send(
                serde_json::json!({"status": "error", "message": "unauthorized"}).to_string(),
            );
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
    };
    let approved_txs = match state.db.get_approved_or_validated_transactions() {
        Ok(txs) => txs,
        Err(e) => {
            tracing::error!("Failed to fetch approved txs: {}", e);
            let _ = state
                .progress_tx
                .send(serde_json::json!({"status": "error", "message": "db error"}).to_string());
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if approved_txs.is_empty() {
        let _ = state
            .progress_tx
            .send(serde_json::json!({"status": "done", "total": 0}).to_string());
        return axum::http::StatusCode::OK.into_response();
    }

    // Group lines by parent tx_id so we do 1 sparse update per transaction entity
    let mut tx_groups: std::collections::HashMap<String, Vec<crate::models::TransactionLine>> =
        std::collections::HashMap::new();
    for line in approved_txs {
        tx_groups.entry(line.tx_id.clone()).or_default().push(line);
    }

    // Spawn a background task to process the writebacks so the HTTP request completes immediately
    tokio::spawn(async move {
        let total_groups = tx_groups.len();
        let mut processed = 0;
        let mut success = 0;
        let mut skipped_or_failed = 0;

        let _ = state.progress_tx.send(
            serde_json::json!({
                "status": "started",
                "total": total_groups,
                "dry_run": payload.dry_run
            })
            .to_string(),
        );

        // We need the QBO class list to map names to QBO IDs
        let active_classes = match qbo_client.fetch_active_classes().await {
            Ok(classes) => classes,
            Err(e) => {
                tracing::error!("Failed to fetch QBO class list for writeback: {}. Aborting batch.", e);
                let _ = state.progress_tx.send(
                    serde_json::json!({"status": "error", "message": format!("Failed to fetch QBO class list: {}", e)}).to_string(),
                );
                return;
            }
        };
        let class_name_to_id: std::collections::HashMap<String, String> = active_classes
            .into_iter()
            .map(|(id, name)| (name, id))
            .collect();

        for (tx_id, lines) in tx_groups {
            // All lines in this group share tx_type and sync_token (we'll just use the first line's type for the endpoint routing)
            let tx_type = lines[0].tx_type.clone();

            tracing::info!("Writing back {} lines for tx_id: {}", lines.len(), tx_id);

            // 1. Fetch the absolute latest entity state to get the most recent SyncToken and avoid schema loss
            let mut entity = match qbo_client.fetch_transaction_by_id(&tx_type, &tx_id).await {
                Ok(e) => e,
                Err(e) => {
                    let err_msg = format!("QBO fetch failed: {}", e);
                    tracing::error!("Writeback fetch failed for {}: {}", tx_id, e);
                    let entity_name = lines
                        .first()
                        .and_then(|l| l.entity_name.clone())
                        .unwrap_or_else(|| "Unknown Entity".to_string());
                    for line in &lines {
                        if let Err(e) = state.db.update_status(
                            &line.tx_id,
                            &line.line_id,
                            TransactionStatus::Failed,
                        ) {
                            tracing::error!("Failed to update status to Failed for {}:{}: {}", line.tx_id, line.line_id, e);
                        }
                    }
                    let _ = state.progress_tx.send(serde_json::json!({"status": "progress", "success": false, "entity_name": entity_name, "error": err_msg }).to_string());
                    skipped_or_failed += 1;
                    processed += 1;
                    let _ = state.progress_tx.send(serde_json::json!({"status": "progress", "processed": processed, "total": total_groups, "success": success, "failed": skipped_or_failed }).to_string());
                    continue;
                }
            };

            // 2. Identify and mutate ClassRef only for the exact line details we approved
            let mut mutated_lines = 0;
            if let Some(entity_lines) = entity.get_mut("Line").and_then(|l| l.as_array_mut()) {
                for entity_line in entity_lines.iter_mut() {
                    let entity_line_id = entity_line
                        .get("Id")
                        .and_then(|id| id.as_str())
                        .unwrap_or("")
                        .to_string();

                    // Match against our approved lines
                    if let Some(approved) = lines.iter().find(|l| l.line_id == entity_line_id) {
                        let class_name = approved.suggested_class_ref.clone().unwrap_or_default();

                        // Look up the QBO ID for this textual class name
                        if let Some(new_class_id) = class_name_to_id.get(&class_name) {
                            // We must inject ClassRef deeply into the correct detail object type.
                            let detail_key =
                                if entity_line.get("AccountBasedExpenseLineDetail").is_some() {
                                    "AccountBasedExpenseLineDetail"
                                } else if entity_line.get("ItemBasedExpenseLineDetail").is_some() {
                                    "ItemBasedExpenseLineDetail"
                                } else if entity_line.get("JournalEntryLineDetail").is_some() {
                                    "JournalEntryLineDetail"
                                } else if entity_line.get("SalesItemLineDetail").is_some() {
                                    "SalesItemLineDetail"
                                } else {
                                    ""
                                };

                            if !detail_key.is_empty()
                                && let Some(detail_obj) = entity_line
                                    .get_mut(detail_key)
                                    .and_then(|d| d.as_object_mut())
                            {
                                detail_obj.insert(
                                    "ClassRef".to_string(),
                                    serde_json::json!({ "value": new_class_id }),
                                );
                                mutated_lines += 1;
                            }
                        } else {
                            tracing::warn!("Could not find QBO ID for class name: {}", class_name);
                        }
                    }
                }
            }

            if mutated_lines == 0 {
                let err_msg = "No matching lines found to mutate (class name not found in QBO or line ID mismatch)";
                tracing::warn!("No matching lines found to mutate in entity {}", tx_id);
                let entity_name = lines
                    .first()
                    .and_then(|l| l.entity_name.clone())
                    .unwrap_or_else(|| "Unknown Entity".to_string());
                for line in &lines {
                    if let Err(e) = state.db.update_status(
                        &line.tx_id,
                        &line.line_id,
                        TransactionStatus::Failed,
                    ) {
                        tracing::error!("Failed to update status to Failed for {}:{}: {}", line.tx_id, line.line_id, e);
                    }
                }
                let _ = state.progress_tx.send(serde_json::json!({"status": "progress", "success": false, "entity_name": entity_name, "error": err_msg }).to_string());
                skipped_or_failed += 1;
                processed += 1;
                let _ = state.progress_tx.send(serde_json::json!({"status": "progress", "processed": processed, "total": total_groups, "success": success, "failed": skipped_or_failed }).to_string());
                continue;
            }

            // Construct payload metadata
            let entity_obj = match entity.as_object_mut() {
                Some(obj) => obj,
                None => {
                    let err_msg = "QBO returned non-object entity JSON — cannot construct sparse update";
                    tracing::error!("Writeback aborted for {}: {}", tx_id, err_msg);
                    let entity_name = lines.first().and_then(|l| l.entity_name.clone()).unwrap_or_else(|| "Unknown Entity".to_string());
                    for line in &lines {
                        if let Err(e) = state.db.update_status(&line.tx_id, &line.line_id, TransactionStatus::Failed) {
                            tracing::error!("Failed to update status to Failed for {}:{}: {}", line.tx_id, line.line_id, e);
                        }
                    }
                    let _ = state.progress_tx.send(serde_json::json!({"status": "progress", "success": false, "entity_name": entity_name, "error": err_msg }).to_string());
                    skipped_or_failed += 1;
                    processed += 1;
                    let _ = state.progress_tx.send(serde_json::json!({"status": "progress", "processed": processed, "total": total_groups, "success": success, "failed": skipped_or_failed }).to_string());
                    continue;
                }
            };
            entity_obj.insert("sparse".to_string(), serde_json::json!(true));
            let request_id = uuid::Uuid::new_v4().to_string();

            // 3. Dry-Run Check vs Actual API Call
            if payload.dry_run {
                let mut is_valid = true;
                let mut validation_errors = Vec::new();

                // Validate QBO minimum sparse update requirements
                let entity_obj = entity.as_object().expect("entity confirmed as object above");
                if !entity_obj.contains_key("Id") {
                    is_valid = false;
                    validation_errors.push("Missing 'Id' field");
                }
                if !entity_obj.contains_key("SyncToken") {
                    is_valid = false;
                    validation_errors.push("Missing 'SyncToken' field");
                }
                if entity_obj.get("sparse").and_then(|v| v.as_bool()) != Some(true) {
                    is_valid = false;
                    validation_errors.push("Missing 'sparse: true' flag");
                }

                // Validate that at least one line has ClassRef
                let mut found_class_ref = false;
                if let Some(lines_arr) = entity_obj.get("Line").and_then(|l| l.as_array()) {
                    for line in lines_arr {
                        for detail_key in &[
                            "AccountBasedExpenseLineDetail",
                            "ItemBasedExpenseLineDetail",
                            "JournalEntryLineDetail",
                            "SalesItemLineDetail",
                        ] {
                            if let Some(detail_obj) =
                                line.get(*detail_key).and_then(|d| d.as_object())
                                && detail_obj.contains_key("ClassRef")
                            {
                                found_class_ref = true;
                            }
                        }
                    }
                }
                if !found_class_ref {
                    is_valid = false;
                    validation_errors.push("Failed to inject 'ClassRef' into any line detail");
                }

                if is_valid {
                    tracing::info!(
                        "DRY RUN: payload for {} generated and validated securely.",
                        tx_id
                    );
                    let req_str = serde_json::to_string(&entity).unwrap_or_default();
                    if let Err(e) = state.db.log_writeback_audit(
                        "batch-dry-run",
                        &req_str,
                        "{\"message\":\"Locally validated successfully against QBO sparse update schema\"}",
                        "DRY_RUN"
                    ) {
                        tracing::error!("Failed to log DRY_RUN audit for {}: {}", tx_id, e);
                    }

                    // Mark as validated locally
                    let entity_name = lines
                        .first()
                        .and_then(|l| l.entity_name.clone())
                        .unwrap_or_else(|| "Unknown Entity".to_string());
                    for approved in &lines {
                        if let Err(e) = state.db.update_status(
                            &approved.tx_id,
                            &approved.line_id,
                            TransactionStatus::Validated,
                        ) {
                            tracing::error!("Failed to update status to Validated for {}:{}: {}", approved.tx_id, approved.line_id, e);
                        }
                    }
                    let _ = state.progress_tx.send(serde_json::json!({"status": "progress", "success": true, "entity_name": entity_name }).to_string());

                    success += 1;
                } else {
                    tracing::error!(
                        "DRY RUN Validation Failed for {}: {:?}",
                        tx_id,
                        validation_errors
                    );
                    let err_msg = format!("Validation failed: {}", validation_errors.join(", "));
                    if let Err(e) = state.db.log_writeback_audit(
                        "batch-dry-run",
                        &serde_json::to_string(&entity).unwrap_or_default(),
                        &serde_json::json!({"error": err_msg}).to_string(),
                        "FAILED",
                    ) {
                        tracing::error!("Failed to log FAILED audit for {}: {}", tx_id, e);
                    }

                    // Mark as failed locally
                    let entity_name = lines
                        .first()
                        .and_then(|l| l.entity_name.clone())
                        .unwrap_or_else(|| "Unknown Entity".to_string());
                    for approved in &lines {
                        if let Err(e) = state.db.update_status(
                            &approved.tx_id,
                            &approved.line_id,
                            TransactionStatus::Failed,
                        ) {
                            tracing::error!("Failed to update status to Failed for {}:{}: {}", approved.tx_id, approved.line_id, e);
                        }
                    }
                    let _ = state.progress_tx.send(serde_json::json!({"status": "progress", "success": false, "entity_name": entity_name, "error": err_msg }).to_string());

                    skipped_or_failed += 1;
                }
            } else {
                match qbo_client
                    .update_transaction_class(&tx_type, &entity, &request_id)
                    .await
                {
                    Ok(response_value) => {
                        let req_str = serde_json::to_string(&entity).unwrap_or_default();
                        let res_str = serde_json::to_string(&response_value).unwrap_or_default();

                        if let Err(e) = state.db.log_writeback_audit(
                            "batch-interactive",
                            &req_str,
                            &res_str,
                            "SUCCESS",
                        ) {
                            tracing::error!("Failed to log SUCCESS audit for {}: {}", tx_id, e);
                        }

                        // Mark as posted locally
                        let entity_name = lines
                            .first()
                            .and_then(|l| l.entity_name.clone())
                            .unwrap_or_else(|| "Unknown Entity".to_string());
                        for approved in &lines {
                            if let Err(e) = state.db.update_status(
                                &approved.tx_id,
                                &approved.line_id,
                                TransactionStatus::Posted,
                            ) {
                                tracing::error!("Failed to update status to Posted for {}:{}: {}", approved.tx_id, approved.line_id, e);
                            }
                        }
                        let _ = state.progress_tx.send(serde_json::json!({"status": "progress", "success": true, "entity_name": entity_name }).to_string());
                        success += 1;
                    }
                    Err(e) => {
                        tracing::error!("Failed to writeback {}: {}", tx_id, e);

                        let req_str = serde_json::to_string(&entity).unwrap_or_default();
                        if let Err(audit_err) = state.db.log_writeback_audit(
                            "batch-interactive",
                            &req_str,
                            &e.to_string(),
                            "FAILED",
                        ) {
                            tracing::error!("Failed to log FAILED audit for {}: {}", tx_id, audit_err);
                        }

                        // Mark as failed locally
                        let entity_name = lines
                            .first()
                            .and_then(|l| l.entity_name.clone())
                            .unwrap_or_else(|| "Unknown Entity".to_string());
                        for approved in &lines {
                            if let Err(db_err) = state.db.update_status(
                                &approved.tx_id,
                                &approved.line_id,
                                TransactionStatus::Failed,
                            ) {
                                tracing::error!("Failed to update status to Failed for {}:{}: {}", approved.tx_id, approved.line_id, db_err);
                            }
                        }
                        let _ = state.progress_tx.send(serde_json::json!({"status": "progress", "success": false, "entity_name": entity_name, "error": e.to_string() }).to_string());

                        skipped_or_failed += 1;
                    }
                }
            }

            processed += 1;
            let _ = state.progress_tx.send(
                serde_json::json!({
                    "status": "progress",
                    "processed": processed,
                    "total": total_groups,
                    "success": success,
                    "failed": skipped_or_failed
                })
                .to_string(),
            );
        }

        let _ = state.progress_tx.send(
            serde_json::json!({
                "status": "done",
                "processed": processed,
                "success": success,
                "failed": skipped_or_failed,
                "dry_run": payload.dry_run
            })
            .to_string(),
        );
    });

    axum::http::StatusCode::ACCEPTED.into_response()
}

// --- Audit Logs Endpoint ---
async fn get_audit_logs(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.get_audit_logs() {
        Ok(logs) => Json(logs).into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch audit logs: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// --- Rules Endpoints ---

async fn get_rules(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.get_rules() {
        Ok(rules) => Json(rules).into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch rules: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

const ALLOWED_RULE_FIELDS: &[&str] = &["account_type", "account", "tx_type", "entity_name"];

async fn create_rule(
    State(state): State<AppState>,
    Json(payload): Json<ClassificationRule>,
) -> impl IntoResponse {
    // Validate condition_field against allowed set to prevent silent no-op rules
    if !ALLOWED_RULE_FIELDS.contains(&payload.condition_field.as_str()) {
        tracing::warn!(
            "Rejected rule with invalid condition_field: '{}'",
            payload.condition_field
        );
        return (axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid condition_field '{}'. Allowed: {:?}", payload.condition_field, ALLOWED_RULE_FIELDS) }))
        ).into_response();
    }

    match state.db.insert_rule(&payload) {
        Ok(_) => {
            tracing::info!(
                "Created rule: IF {} = '{}' THEN '{}'",
                payload.condition_field,
                payload.condition_value,
                payload.target_class
            );
            axum::http::StatusCode::CREATED.into_response()
        }
        Err(e) => {
            tracing::error!("Failed to create rule: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
struct DeleteRuleRequest {
    id: i64,
}

async fn delete_rule(
    State(state): State<AppState>,
    Json(payload): Json<DeleteRuleRequest>,
) -> impl IntoResponse {
    match state.db.delete_rule(payload.id) {
        Ok(_) => {
            tracing::info!("Deleted rule ID: {}", payload.id);
            axum::http::StatusCode::OK.into_response()
        }
        Err(e) => {
            tracing::error!("Failed to delete rule: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// --- Classes Endpoint ---

async fn get_valid_qbo_client() -> Result<QboClient, String> {
    let (access_token, refresh_token, realm_id) =
        secrets::get_oauth_data().map_err(|e| e.to_string())?;
    let (mut qbo_client, token_expired) = {
        let client = QboClient::new(realm_id.clone(), access_token);
        let expired = match client.fetch_active_classes().await {
            Ok(_) => false,
            Err(e) => {
                if e.to_string().contains("AuthenticationFailed") {
                    true
                } else {
                    return Err(e.to_string());
                }
            }
        };
        (client, expired)
    };

    if token_expired {
        tracing::info!("Access token expired. Refreshing OAuth token...");
        let (client_id, client_secret) =
            secrets::get_client_credentials().map_err(|e| e.to_string())?;
        let tokens = QboClient::refresh_oauth_token(&client_id, &client_secret, &refresh_token)
            .await
            .map_err(|e| e.to_string())?;
        secrets::store_oauth_data(&tokens.access_token, &tokens.refresh_token, &realm_id)
            .map_err(|e| e.to_string())?;
        qbo_client.update_access_token(tokens.access_token);
    }

    Ok(qbo_client)
}

async fn get_available_classes() -> impl IntoResponse {
    // Fetch active QBO classes for the dropdown
    let result: Result<Vec<(String, String)>, String> = async {
        let qbo_client = get_valid_qbo_client().await?;
        qbo_client
            .fetch_active_classes()
            .await
            .map_err(|e| e.to_string())
    }
    .await;

    match result {
        Ok(classes) => Json(classes).into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch QBO classes: {}", e);
            (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": format!("Failed to fetch QBO classes: {}", e)}))).into_response()
        }
    }
}

// --- OAuth Endpoints ---

async fn oauth_login(State(state): State<AppState>) -> impl IntoResponse {
    let creds = match secrets::get_client_credentials() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to retrieve client credentials from Keychain: {}", e);
            return Redirect::temporary("/?oauth_error=no_creds").into_response();
        }
    };

    let client_id = creds.0;
    let redirect_uri = OAUTH_REDIRECT_URI;
    let scope = "com.intuit.quickbooks.accounting";

    // Generate a cryptographically random CSRF nonce (Intuit Security: strong entropy)
    let csrf_state: String = (0..32)
        .map(|_| format!("{:02x}", rand::random::<u8>()))
        .collect();
    if let Ok(mut guard) = state.oauth_state.lock() {
        *guard = Some(csrf_state.clone());
    }

    let auth_url = format!(
        "https://appcenter.intuit.com/connect/oauth2?client_id={}&response_type=code&scope={}&redirect_uri={}&state={}",
        client_id,
        encode(scope),
        encode(redirect_uri),
        encode(&csrf_state)
    );

    Redirect::temporary(&auth_url).into_response()
}

#[derive(Deserialize)]
struct OauthCallbackParams {
    code: Option<String>,
    #[serde(rename = "realmId")]
    realm_id: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn oauth_callback(
    State(app_state): State<AppState>,
    Query(params): Query<OauthCallbackParams>,
) -> impl IntoResponse {
    // Intuit Security: /callback receives sensitive tokens as URL params.
    // All paths must return a 302 redirect, never HTML, to prevent Referer leakage.

    if let Some(ref err) = params.error {
        tracing::error!("OAuth Error: {}", err);
        return Redirect::temporary("/?oauth_error=true").into_response();
    }

    // Verify CSRF state parameter — reject if state is missing or mismatched
    let expected_state = app_state.oauth_state.lock().ok().and_then(|mut g| g.take());
    if expected_state.is_none() {
        tracing::error!("OAuth CSRF state not found (server may have restarted). Rejecting callback.");
        return Redirect::temporary("/?oauth_error=csrf").into_response();
    }
    if params.state != expected_state {
        tracing::error!(
            "OAuth CSRF state mismatch. Expected: {:?}, Got: {:?}",
            expected_state,
            params.state
        );
        return Redirect::temporary("/?oauth_error=csrf").into_response();
    }

    if let (Some(code), Some(realm)) = (params.code, params.realm_id) {
        let creds = match secrets::get_client_credentials() {
            Ok(c) => c,
            Err(_) => return Redirect::temporary("/?oauth_error=no_creds").into_response(),
        };

        match QboClient::exchange_oauth_token(&creds.0, &creds.1, &code, OAUTH_REDIRECT_URI).await {
            Ok(tokens) => {
                match secrets::store_oauth_data(&tokens.access_token, &tokens.refresh_token, &realm)
                {
                    Ok(_) => {
                        tracing::info!(
                            "OAuth tokens securely cached in macOS Keychain for Realm: {}",
                            realm
                        );
                        Redirect::temporary("/?oauth=success").into_response()
                    }
                    Err(e) => {
                        tracing::error!("Failed to store token in Keychain: {}", e);
                        Redirect::temporary("/?oauth_error=storage").into_response()
                    }
                }
            }
            Err(e) => {
                tracing::error!("Token exchange failed: {}", e);
                Redirect::temporary("/?oauth_error=exchange").into_response()
            }
        }
    } else {
        Redirect::temporary("/?oauth_error=invalid").into_response()
    }
}

async fn oauth_disconnect() -> impl IntoResponse {
    match secrets::delete_oauth_data() {
        Ok(_) => {
            tracing::info!("OAuth tokens cleared from Keychain (Disconnected).");
            Redirect::temporary("/?disconnected=true").into_response()
        }
        Err(e) => {
            tracing::error!("Failed to clear OAuth tokens: {}", e);
            Redirect::temporary("/?error=disconnect_failed").into_response()
        }
    }
}

// --- Diagnostics Endpoints ---

async fn get_diagnostics_status() -> impl IntoResponse {
    let result = secrets::get_oauth_data();
    match result {
        Ok((_, _, realm)) => Json(serde_json::json!({
            "status": "connected",
            "realm_id": realm,
        })),
        Err(_) => Json(serde_json::json!({
            "status": "disconnected",
            "realm_id": null,
        })),
    }
}

async fn get_company_info() -> impl IntoResponse {
    let result: Result<Value, String> = async {
        let qbo_client = get_valid_qbo_client().await?;
        qbo_client
            .fetch_company_info()
            .await
            .map_err(|e| e.to_string())
    }
    .await;

    match result {
        Ok(info) => Json::<Value>(info).into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch CompanyInfo: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    }
}

async fn refresh_token_manual() -> impl IntoResponse {
    let result: Result<(), String> = async {
        let (_access_token, refresh_token, realm_id) =
            secrets::get_oauth_data().map_err(|e| e.to_string())?;

        tracing::info!("Manual token refresh triggered...");
        let (client_id, client_secret) =
            secrets::get_client_credentials().map_err(|e| e.to_string())?;

        let tokens = QboClient::refresh_oauth_token(&client_id, &client_secret, &refresh_token)
            .await
            .map_err(|e| e.to_string())?;

        secrets::store_oauth_data(&tokens.access_token, &tokens.refresh_token, &realm_id)
            .map_err(|e| e.to_string())?;

        Ok(())
    }
    .await;

    match result {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "message": "Token refreshed successfully" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct SeedTokensRequest {
    access_token: String,
    refresh_token: String,
    realm_id: String,
}

async fn seed_tokens_manual(Json(payload): Json<SeedTokensRequest>) -> impl IntoResponse {
    match secrets::store_oauth_data(
        &payload.access_token,
        &payload.refresh_token,
        &payload.realm_id,
    ) {
        Ok(_) => {
            tracing::info!(
                "OAuth tokens manually seeded into Keychain for Realm: {}",
                payload.realm_id
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({ "message": "Tokens saved to Keychain" })),
            )
        }
        Err(e) => {
            tracing::error!("Failed to seed tokens: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}
