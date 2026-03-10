// Purpose: Core domains and JSON schemas for API interactions and database entities.
// Owner: Antigravity Agent
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransactionLine {
    pub tx_id: String,
    pub line_id: String,
    pub sync_token: String,
    pub tx_type: String,
    pub tx_date: Option<String>,
    pub entity_name: Option<String>,
    pub header_memo: Option<String>,
    pub line_description: Option<String>,
    pub account: Option<String>,
    pub account_type: Option<String>,
    pub po_ref: Option<String>,
    pub amount: Option<f64>,
    pub suggested_class_ref: Option<String>,
    pub llm_reasoning: Option<String>,
    pub confidence_score: Option<f32>,
    pub status: TransactionStatus,
}

/// Aggregated bucket for grouped review UI.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransactionBucket {
    pub account: String,
    pub entity_name: String,
    pub suggested_class_ref: String,
    pub tx_count: i64,
    pub total_amount: f64,
    pub avg_confidence: f64,
}

/// User-defined classification rule (heuristic).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClassificationRule {
    pub id: Option<i64>,
    pub condition_field: String, // e.g. "account_type"
    pub condition_value: String, // e.g. "Accounts Payable"
    pub target_class: String,    // e.g. "Balance Sheet"
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum TransactionStatus {
    Pending,
    Approved,
    Rejected,
    Validated,
    Failed,
    Posted,
}

impl std::fmt::Display for TransactionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TransactionStatus::Pending => "Pending",
            TransactionStatus::Approved => "Approved",
            TransactionStatus::Rejected => "Rejected",
            TransactionStatus::Validated => "Validated",
            TransactionStatus::Failed => "Failed",
            TransactionStatus::Posted => "Posted",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for TransactionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Pending" => Ok(TransactionStatus::Pending),
            "Approved" => Ok(TransactionStatus::Approved),
            "Rejected" => Ok(TransactionStatus::Rejected),
            "Validated" => Ok(TransactionStatus::Validated),
            "Failed" => Ok(TransactionStatus::Failed),
            "Posted" => Ok(TransactionStatus::Posted),
            _ => Err(format!("Invalid status: {}", s)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LlmPrediction {
    pub class_ref: String,
    pub reasoning: String,
    pub confidence_score: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditLog {
    pub id: String,
    pub batch_id: String,
    pub request_json: String,
    pub response_json: String,
    pub status: String,
    pub timestamp: String,
}
