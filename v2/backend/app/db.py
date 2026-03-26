"""SQLite persistence layer. Ported from Rust src/db.rs.
Uses aiosqlite for async access with the same schema and query patterns.
"""
import logging
import uuid
from datetime import datetime

import aiosqlite

from .models import (
    AuditLog, ClassificationRule, TransactionBucket,
    TransactionLine, TransactionStatus, STATUS_TRANSITIONS,
)

logger = logging.getLogger(__name__)

DB_PATH = "reclass_utility.db"


async def init_db() -> "AppDb":
    conn = await aiosqlite.connect(DB_PATH)
    conn.row_factory = aiosqlite.Row

    # SQLite hardening
    await conn.execute("PRAGMA journal_mode=WAL")
    await conn.execute("PRAGMA busy_timeout=5000")
    await conn.execute("PRAGMA foreign_keys=ON")

    await conn.execute("""
        CREATE TABLE IF NOT EXISTS transaction_lines (
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
        )
    """)

    await conn.execute("""
        CREATE TABLE IF NOT EXISTS classification_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            condition_field TEXT NOT NULL,
            condition_value TEXT NOT NULL,
            target_class TEXT NOT NULL
        )
    """)

    await conn.execute("""
        CREATE TABLE IF NOT EXISTS writeback_audit (
            id TEXT PRIMARY KEY,
            batch_id TEXT NOT NULL,
            request_json TEXT NOT NULL,
            response_json TEXT NOT NULL,
            status TEXT NOT NULL,
            timestamp TEXT NOT NULL
        )
    """)

    # Idempotent migrations
    for col, typ in [("amount", "REAL"), ("tx_date", "TEXT"), ("account_type", "TEXT")]:
        try:
            await conn.execute(f"ALTER TABLE transaction_lines ADD COLUMN {col} {typ}")
        except Exception:
            pass  # Column already exists

    await conn.commit()
    return AppDb(conn)


class AppDb:
    def __init__(self, conn: aiosqlite.Connection):
        self.conn = conn

    async def insert_transaction(self, tx: TransactionLine) -> None:
        await self.conn.execute(
            """INSERT INTO transaction_lines (
                tx_id, line_id, sync_token, tx_type, tx_date, entity_name, header_memo,
                line_description, account, account_type, po_ref, amount,
                suggested_class_ref, llm_reasoning, confidence_score, status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
            WHERE transaction_lines.status = 'Pending'""",
            (
                tx.tx_id, tx.line_id, tx.sync_token, tx.tx_type, tx.tx_date,
                tx.entity_name, tx.header_memo, tx.line_description, tx.account,
                tx.account_type, tx.po_ref, tx.amount, tx.suggested_class_ref,
                tx.llm_reasoning, tx.confidence_score, tx.status.value,
            ),
        )
        await self.conn.commit()

    async def update_inference(
        self, tx_id: str, line_id: str, class_ref: str, reasoning: str, confidence: float
    ) -> None:
        await self.conn.execute(
            """UPDATE transaction_lines
               SET suggested_class_ref = ?, llm_reasoning = ?, confidence_score = ?
               WHERE tx_id = ? AND line_id = ? AND status = 'Pending'""",
            (class_ref, reasoning, confidence, tx_id, line_id),
        )
        await self.conn.commit()

    async def _query_transactions(self, where_clause: str, params: tuple = ()) -> list[TransactionLine]:
        rows = await self.conn.execute_fetchall(
            f"""SELECT tx_id, line_id, sync_token, tx_type, tx_date, entity_name, header_memo,
                line_description, account, account_type, po_ref, amount,
                suggested_class_ref, llm_reasoning, confidence_score, status
                FROM transaction_lines {where_clause}
                ORDER BY tx_date, tx_id, line_id""",
            params,
        )
        return [
            TransactionLine(
                tx_id=r[0], line_id=r[1], sync_token=r[2], tx_type=r[3],
                tx_date=r[4], entity_name=r[5], header_memo=r[6],
                line_description=r[7], account=r[8], account_type=r[9],
                po_ref=r[10], amount=r[11], suggested_class_ref=r[12],
                llm_reasoning=r[13], confidence_score=r[14],
                status=TransactionStatus(r[15]) if r[15] in TransactionStatus.__members__ else TransactionStatus.Pending,
            )
            for r in rows
        ]

    async def get_pending_transactions(self) -> list[TransactionLine]:
        return await self._query_transactions("WHERE status = 'Pending'")

    async def get_approved_or_validated_transactions(self) -> list[TransactionLine]:
        return await self._query_transactions("WHERE status IN ('Approved', 'Validated')")

    async def get_grouped_transactions(self, status: str) -> list[TransactionBucket]:
        rows = await self.conn.execute_fetchall(
            """SELECT
                COALESCE(account, 'Unknown') as acct,
                COALESCE(entity_name, 'Unknown') as entity,
                COALESCE(suggested_class_ref, 'Unclassified') as class,
                COUNT(*) as cnt,
                COALESCE(SUM(amount), 0) as total,
                COALESCE(AVG(confidence_score), 0) as avg_conf
             FROM transaction_lines
             WHERE status = ?
             GROUP BY acct, entity, class
             ORDER BY entity, acct, class""",
            (status,),
        )
        return [
            TransactionBucket(
                account=r[0], entity_name=r[1], suggested_class_ref=r[2],
                tx_count=r[3], total_amount=r[4], avg_confidence=r[5],
            )
            for r in rows
        ]

    async def get_group_items(
        self, status: str, account: str, entity_name: str, suggested_class: str
    ) -> list[TransactionLine]:
        return await self._query_transactions(
            """WHERE status = ?
               AND COALESCE(account, 'Unknown') = ?
               AND COALESCE(entity_name, 'Unknown') = ?
               AND COALESCE(suggested_class_ref, 'Unclassified') = ?""",
            (status, account, entity_name, suggested_class),
        )

    async def batch_approve_group(
        self, account: str, entity_name: str, current_class: str, override_class: str | None
    ) -> int:
        # Atomic: override class + approve in a single transaction
        if override_class:
            await self.conn.execute(
                """UPDATE transaction_lines
                   SET suggested_class_ref = ?, llm_reasoning = 'User override via batch approval'
                   WHERE status = 'Pending'
                     AND COALESCE(account, 'Unknown') = ?
                     AND COALESCE(entity_name, 'Unknown') = ?
                     AND COALESCE(suggested_class_ref, 'Unclassified') = ?""",
                (override_class, account, entity_name, current_class),
            )

        final_class = override_class or current_class
        cursor = await self.conn.execute(
            """UPDATE transaction_lines
               SET status = 'Approved'
               WHERE status = 'Pending'
                 AND COALESCE(account, 'Unknown') = ?
                 AND COALESCE(entity_name, 'Unknown') = ?
                 AND COALESCE(suggested_class_ref, 'Unclassified') = ?""",
            (account, entity_name, final_class),
        )
        await self.conn.commit()
        return cursor.rowcount

    async def update_status(self, tx_id: str, line_id: str, new_status: TransactionStatus) -> int:
        allowed_from = STATUS_TRANSITIONS.get(new_status, [])
        placeholders = ",".join(f"'{s.value}'" for s in allowed_from)
        cursor = await self.conn.execute(
            f"""UPDATE transaction_lines SET status = ?
                WHERE tx_id = ? AND line_id = ? AND status IN ({placeholders})""",
            (new_status.value, tx_id, line_id),
        )
        await self.conn.commit()
        if cursor.rowcount == 0:
            logger.warning(
                "Status update to %s had no effect for %s:%s — transition may not be allowed",
                new_status, tx_id, line_id,
            )
        return cursor.rowcount

    async def reset_failed_group_to_pending(
        self, account: str, entity_name: str, class_ref: str
    ) -> int:
        cursor = await self.conn.execute(
            """UPDATE transaction_lines SET status = 'Pending'
               WHERE status = 'Failed'
                 AND COALESCE(account, 'Unknown') = ?
                 AND COALESCE(entity_name, 'Unknown') = ?
                 AND COALESCE(suggested_class_ref, 'Unclassified') = ?""",
            (account, entity_name, class_ref),
        )
        await self.conn.commit()
        return cursor.rowcount

    async def log_writeback_audit(
        self, batch_id: str, request_json: str, response_json: str, status: str
    ) -> None:
        audit_id = str(uuid.uuid4())
        timestamp = datetime.now().isoformat()
        await self.conn.execute(
            """INSERT INTO writeback_audit (id, batch_id, request_json, response_json, status, timestamp)
               VALUES (?, ?, ?, ?, ?, ?)""",
            (audit_id, batch_id, request_json, response_json, status, timestamp),
        )
        await self.conn.commit()

    async def get_audit_logs(self) -> list[AuditLog]:
        rows = await self.conn.execute_fetchall(
            "SELECT id, batch_id, request_json, response_json, status, timestamp FROM writeback_audit ORDER BY timestamp DESC"
        )
        return [
            AuditLog(id=r[0], batch_id=r[1], request_json=r[2], response_json=r[3], status=r[4], timestamp=r[5])
            for r in rows
        ]

    # --- Rules ---

    async def get_rules(self) -> list[ClassificationRule]:
        rows = await self.conn.execute_fetchall(
            "SELECT id, condition_field, condition_value, target_class FROM classification_rules ORDER BY id"
        )
        return [
            ClassificationRule(id=r[0], condition_field=r[1], condition_value=r[2], target_class=r[3])
            for r in rows
        ]

    async def insert_rule(self, rule: ClassificationRule) -> None:
        await self.conn.execute(
            "INSERT INTO classification_rules (condition_field, condition_value, target_class) VALUES (?, ?, ?)",
            (rule.condition_field, rule.condition_value, rule.target_class),
        )
        await self.conn.commit()

    async def delete_rule(self, rule_id: int) -> None:
        await self.conn.execute("DELETE FROM classification_rules WHERE id = ?", (rule_id,))
        await self.conn.commit()
