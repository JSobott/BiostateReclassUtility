// Purpose: SQLite persistence layer for transaction line items and classification rules.
// Owner: Antigravity Agent
use crate::models::{
    AuditLog, ClassificationRule, TransactionBucket, TransactionLine, TransactionStatus,
};
use rusqlite::{Connection, Result, params};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppDb {
    conn: Arc<Mutex<Connection>>,
}

pub fn init_db() -> Result<AppDb, Box<dyn std::error::Error>> {
    let conn = Connection::open("reclass_utility.db")?;

    // SQLite hardening: WAL mode for crash safety, busy timeout for concurrent access
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA busy_timeout=5000;
         PRAGMA foreign_keys=ON;"
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS transaction_lines (
            tx_id TEXT NOT NULL,
            line_id TEXT NOT NULL,
            sync_token TEXT NOT NULL,
            tx_type TEXT NOT NULL,
            tx_date TEXT,
            entity_name TEXT,
            header_memo TEXT,
            line_description TEXT,
            account TEXT,
            account_type TEXT,
            po_ref TEXT,
            amount REAL,
            suggested_class_ref TEXT,
            llm_reasoning TEXT,
            confidence_score REAL,
            status TEXT NOT NULL,
            PRIMARY KEY (tx_id, line_id)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS classification_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            condition_field TEXT NOT NULL,
            condition_value TEXT NOT NULL,
            target_class TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS writeback_audit (
            id TEXT PRIMARY KEY,
            batch_id TEXT NOT NULL,
            request_json TEXT NOT NULL,
            response_json TEXT NOT NULL,
            status TEXT NOT NULL,
            timestamp TEXT NOT NULL
        )",
        [],
    )?;

    // Idempotent migrations for existing databases
    let _ = conn.execute("ALTER TABLE transaction_lines ADD COLUMN amount REAL", []);
    let _ = conn.execute("ALTER TABLE transaction_lines ADD COLUMN tx_date TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE transaction_lines ADD COLUMN account_type TEXT",
        [],
    );

    Ok(AppDb {
        conn: Arc::new(Mutex::new(conn)),
    })
}

impl AppDb {
    /// Locks the inner connection, converting a poisoned Mutex into a rusqlite error
    /// instead of panicking the entire server.
    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|e| {
            tracing::error!("Database Mutex poisoned — a thread panicked while holding the DB lock: {}", e);
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                Some("Database Mutex poisoned".to_string()),
            )
        })
    }

    pub fn insert_transaction(&self, tx: &TransactionLine) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT INTO transaction_lines (
                tx_id, line_id, sync_token, tx_type, tx_date, entity_name, header_memo,
                line_description, account, account_type, po_ref, amount,
                suggested_class_ref, llm_reasoning, confidence_score, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            ON CONFLICT(tx_id, line_id) DO UPDATE SET
                sync_token = excluded.sync_token,
                tx_type = excluded.tx_type,
                tx_date = excluded.tx_date,
                entity_name = excluded.entity_name,
                header_memo = excluded.header_memo,
                line_description = excluded.line_description,
                account = excluded.account,
                account_type = excluded.account_type,
                po_ref = excluded.po_ref,
                amount = excluded.amount,
                suggested_class_ref = COALESCE(excluded.suggested_class_ref, transaction_lines.suggested_class_ref),
                llm_reasoning = COALESCE(excluded.llm_reasoning, transaction_lines.llm_reasoning),
                confidence_score = COALESCE(excluded.confidence_score, transaction_lines.confidence_score)
            WHERE transaction_lines.status = 'Pending'",
            params![
                tx.tx_id,
                tx.line_id,
                tx.sync_token,
                tx.tx_type,
                tx.tx_date,
                tx.entity_name,
                tx.header_memo,
                tx.line_description,
                tx.account,
                tx.account_type,
                tx.po_ref,
                tx.amount,
                tx.suggested_class_ref,
                tx.llm_reasoning,
                tx.confidence_score,
                tx.status.to_string(),
            ],
        )?;
        Ok(())
    }

    /// Update only the LLM inference fields after classification runs.
    /// Guarded: only updates rows still in Pending status to prevent overwriting user approvals.
    pub fn update_inference(
        &self,
        tx_id: &str,
        line_id: &str,
        class_ref: &str,
        reasoning: &str,
        confidence: f32,
    ) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute(
            "UPDATE transaction_lines
             SET suggested_class_ref = ?1, llm_reasoning = ?2, confidence_score = ?3
             WHERE tx_id = ?4 AND line_id = ?5 AND status = 'Pending'",
            params![class_ref, reasoning, confidence, tx_id, line_id],
        )?;
        Ok(())
    }

    pub fn get_pending_transactions(&self) -> Result<Vec<TransactionLine>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT tx_id, line_id, sync_token, tx_type, tx_date, entity_name, header_memo,
             line_description, account, account_type, po_ref, amount,
             suggested_class_ref, llm_reasoning, confidence_score, status
             FROM transaction_lines WHERE status = 'Pending'
             ORDER BY tx_date, tx_id, line_id",
        )?;

        let tx_iter = stmt.query_map([], |row| {
            let status_str: String = row.get(15)?;
            let status = status_str.parse().unwrap_or(TransactionStatus::Pending);
            Ok(TransactionLine {
                tx_id: row.get(0)?,
                line_id: row.get(1)?,
                sync_token: row.get(2)?,
                tx_type: row.get(3)?,
                tx_date: row.get(4)?,
                entity_name: row.get(5)?,
                header_memo: row.get(6)?,
                line_description: row.get(7)?,
                account: row.get(8)?,
                account_type: row.get(9)?,
                po_ref: row.get(10)?,
                amount: row.get(11)?,
                suggested_class_ref: row.get(12)?,
                llm_reasoning: row.get(13)?,
                confidence_score: row.get(14)?,
                status,
            })
        })?;

        let mut transactions = Vec::new();
        for tx in tx_iter {
            transactions.push(tx?);
        }
        Ok(transactions)
    }

    /// Fetches all lines that have been 'Approved' or 'Validated' explicitly.
    pub fn get_approved_or_validated_transactions(&self) -> Result<Vec<TransactionLine>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT tx_id, line_id, sync_token, tx_type, tx_date, entity_name, header_memo,
             line_description, account, account_type, po_ref, amount,
             suggested_class_ref, llm_reasoning, confidence_score, status
             FROM transaction_lines WHERE status IN ('Approved', 'Validated')
             ORDER BY tx_date, tx_id, line_id",
        )?;

        let tx_iter = stmt.query_map([], |row| {
            let status_str: String = row.get(15)?;
            let status = status_str.parse().unwrap_or(TransactionStatus::Approved);
            Ok(TransactionLine {
                tx_id: row.get(0)?,
                line_id: row.get(1)?,
                sync_token: row.get(2)?,
                tx_type: row.get(3)?,
                tx_date: row.get(4)?,
                entity_name: row.get(5)?,
                header_memo: row.get(6)?,
                line_description: row.get(7)?,
                account: row.get(8)?,
                account_type: row.get(9)?,
                po_ref: row.get(10)?,
                amount: row.get(11)?,
                suggested_class_ref: row.get(12)?,
                llm_reasoning: row.get(13)?,
                confidence_score: row.get(14)?,
                status,
            })
        })?;

        let mut transactions = Vec::new();
        for tx in tx_iter {
            transactions.push(tx?);
        }
        Ok(transactions)
    }

    /// Resets a specific failed transaction group back to 'Pending' so it can be re-run by the pipeline.
    pub fn reset_failed_group_to_pending(
        &self,
        account: &str,
        entity: &str,
        class: &str,
    ) -> Result<usize> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "UPDATE transaction_lines
             SET status = 'Pending'
             WHERE status = 'Failed'
             AND COALESCE(account, 'Unknown') = ?1
             AND COALESCE(entity_name, 'Unknown') = ?2
             AND COALESCE(suggested_class_ref, 'Unclassified') = ?3",
        )?;
        let count = stmt.execute(params![account, entity, class])?;
        Ok(count)
    }

    /// Returns aggregated buckets grouped by (account, entity_name, suggested_class_ref) for a given status.
    pub fn get_grouped_transactions(&self, status: &str) -> Result<Vec<TransactionBucket>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT
                COALESCE(account, 'Unknown') as acct,
                COALESCE(entity_name, 'Unknown') as entity,
                COALESCE(suggested_class_ref, 'Unclassified') as class,
                COUNT(*) as cnt,
                COALESCE(SUM(amount), 0) as total,
                COALESCE(AVG(confidence_score), 0) as avg_conf
             FROM transaction_lines
             WHERE status = ?1
             GROUP BY acct, entity, class
             ORDER BY entity, acct, class",
        )?;

        let rows = stmt.query_map(params![status], |row| {
            Ok(TransactionBucket {
                account: row.get(0)?,
                entity_name: row.get(1)?,
                suggested_class_ref: row.get(2)?,
                tx_count: row.get(3)?,
                total_amount: row.get(4)?,
                avg_confidence: row.get(5)?,
            })
        })?;

        let mut buckets = Vec::new();
        for row in rows {
            buckets.push(row?);
        }
        Ok(buckets)
    }

    /// Returns individual transactions matching a group's filter for a specific status.
    pub fn get_group_items(
        &self,
        status: &str,
        account: &str,
        entity_name: &str,
        suggested_class: &str,
    ) -> Result<Vec<TransactionLine>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT tx_id, line_id, sync_token, tx_type, tx_date, entity_name, header_memo,
             line_description, account, account_type, po_ref, amount,
             suggested_class_ref, llm_reasoning, confidence_score, status
             FROM transaction_lines
             WHERE status = ?1
               AND COALESCE(account, 'Unknown') = ?2
               AND COALESCE(entity_name, 'Unknown') = ?3
               AND COALESCE(suggested_class_ref, 'Unclassified') = ?4
             ORDER BY tx_date, tx_id, line_id",
        )?;

        let rows = stmt.query_map(
            params![status, account, entity_name, suggested_class],
            |row| {
                let status_str: String = row.get(15)?;
                let mapped_status = status_str.parse().unwrap_or(TransactionStatus::Pending);
                Ok(TransactionLine {
                    tx_id: row.get(0)?,
                    line_id: row.get(1)?,
                    sync_token: row.get(2)?,
                    tx_type: row.get(3)?,
                    tx_date: row.get(4)?,
                    entity_name: row.get(5)?,
                    header_memo: row.get(6)?,
                    line_description: row.get(7)?,
                    account: row.get(8)?,
                    account_type: row.get(9)?,
                    po_ref: row.get(10)?,
                    amount: row.get(11)?,
                    suggested_class_ref: row.get(12)?,
                    llm_reasoning: row.get(13)?,
                    confidence_score: row.get(14)?,
                    status: mapped_status,
                })
            },
        )?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    /// Batch-approve all pending transactions matching account + entity_name + current suggested class.
    /// If override_class is provided, update the suggested_class_ref before approving.
    pub fn batch_approve_group(
        &self,
        account: &str,
        entity_name: &str,
        current_class: &str,
        override_class: Option<&str>,
    ) -> Result<usize> {
        let conn = self.lock_conn()?;

        // Wrap override + approve in a single transaction for atomicity
        let tx = conn.unchecked_transaction()?;

        // If overriding, update the class first
        if let Some(new_class) = override_class {
            tx.execute(
                "UPDATE transaction_lines
                 SET suggested_class_ref = ?1, llm_reasoning = 'User override via batch approval'
                 WHERE status = 'Pending'
                   AND COALESCE(account, 'Unknown') = ?2
                   AND COALESCE(entity_name, 'Unknown') = ?3
                   AND COALESCE(suggested_class_ref, 'Unclassified') = ?4",
                params![new_class, account, entity_name, current_class],
            )?;
        }

        // Approve all matching rows
        let updated = tx.execute(
            "UPDATE transaction_lines
             SET status = 'Approved'
             WHERE status = 'Pending'
               AND COALESCE(account, 'Unknown') = ?1
               AND COALESCE(entity_name, 'Unknown') = ?2
               AND COALESCE(suggested_class_ref, 'Unclassified') = ?3",
            params![
                account,
                entity_name,
                override_class.unwrap_or(current_class)
            ],
        )?;

        tx.commit()?;
        Ok(updated)
    }

    /// Updates the status of a transaction line. Includes a guard to prevent
    /// invalid backwards transitions (e.g., Posted -> Pending).
    pub fn update_status(
        &self,
        tx_id: &str,
        line_id: &str,
        status: TransactionStatus,
    ) -> Result<usize> {
        let conn = self.lock_conn()?;
        // Guard: only allow valid forward transitions.
        // Pending -> Approved, Approved -> Validated/Failed, Validated -> Posted/Failed, Failed -> Pending (reset)
        let allowed_from = match status {
            TransactionStatus::Approved => "'Pending'",
            TransactionStatus::Validated => "'Approved'",
            TransactionStatus::Posted => "'Approved', 'Validated'",
            TransactionStatus::Failed => "'Approved', 'Validated'",
            TransactionStatus::Rejected => "'Pending'",
            TransactionStatus::Pending => "'Failed'", // Only allow reset from Failed
        };
        let sql = format!(
            "UPDATE transaction_lines SET status = ?1 WHERE tx_id = ?2 AND line_id = ?3 AND status IN ({})",
            allowed_from
        );
        let count = conn.execute(&sql, params![status.to_string(), tx_id, line_id])?;
        if count == 0 {
            tracing::warn!(
                "Status update to {:?} had no effect for {}:{} — row may not exist or transition is not allowed from current status",
                status, tx_id, line_id
            );
        }
        Ok(count)
    }

    /// Logs a QBO sparse update execution for audit requirements
    pub fn log_writeback_audit(
        &self,
        batch_id: &str,
        request_json: &str,
        response_json: &str,
        status: &str,
    ) -> Result<()> {
        let conn = self.lock_conn()?;
        let id = uuid::Uuid::new_v4().to_string();
        let timestamp = chrono::Local::now().to_rfc3339();

        conn.execute(
            "INSERT INTO writeback_audit (id, batch_id, request_json, response_json, status, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, batch_id, request_json, response_json, status, timestamp],
        )?;
        Ok(())
    }

    pub fn get_audit_logs(&self) -> Result<Vec<AuditLog>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, batch_id, request_json, response_json, status, timestamp
             FROM writeback_audit
             ORDER BY timestamp DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(AuditLog {
                id: row.get(0)?,
                batch_id: row.get(1)?,
                request_json: row.get(2)?,
                response_json: row.get(3)?,
                status: row.get(4)?,
                timestamp: row.get(5)?,
            })
        })?;

        let mut logs = Vec::new();
        for row in rows {
            logs.push(row?);
        }
        Ok(logs)
    }

    // --- Classification Rules CRUD ---

    pub fn get_rules(&self) -> Result<Vec<ClassificationRule>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, condition_field, condition_value, target_class FROM classification_rules ORDER BY id"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(ClassificationRule {
                id: row.get(0)?,
                condition_field: row.get(1)?,
                condition_value: row.get(2)?,
                target_class: row.get(3)?,
            })
        })?;

        let mut rules = Vec::new();
        for row in rows {
            rules.push(row?);
        }
        Ok(rules)
    }

    pub fn insert_rule(&self, rule: &ClassificationRule) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT INTO classification_rules (condition_field, condition_value, target_class)
             VALUES (?1, ?2, ?3)",
            params![
                rule.condition_field,
                rule.condition_value,
                rule.target_class
            ],
        )?;
        Ok(())
    }

    pub fn delete_rule(&self, rule_id: i64) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute(
            "DELETE FROM classification_rules WHERE id = ?1",
            params![rule_id],
        )?;
        Ok(())
    }
}
