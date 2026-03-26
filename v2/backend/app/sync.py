"""Live QBO data ingestion and LLM-inference pipeline (via General Ledger report).
Ported from Rust src/sync.rs.
"""
import asyncio
import logging
from dataclasses import dataclass
from datetime import date, timedelta
from typing import Any, Callable

from .config import (
    GL_CHUNK_DAYS,
    LLM_CONCURRENT_REQUESTS,
    QBO_CONCURRENT_FETCHES,
    QBO_FETCH_DELAY_MS,
    REPORT_TYPE_MAP,
)
from .db import AppDb
from .models import ClassificationRule, TransactionLine, TransactionStatus
from .qbo_client import AuthenticationError, QboClient
from . import llm_engine, secrets

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# GlEntry — intermediate struct for GL posting data before entity fetch
# ---------------------------------------------------------------------------

@dataclass
class GlEntry:
    tx_id: str
    api_type: str
    gl_account: str
    gl_account_type: str | None
    gl_date: str | None
    gl_amount: float | None
    gl_memo: str | None


# ---------------------------------------------------------------------------
# Main sync entry point
# ---------------------------------------------------------------------------

async def run_sync_job(
    db: AppDb,
    start_date: str | None = None,
    end_date: str | None = None,
    progress_callback: Callable[[dict], None] | None = None,
) -> None:
    """Run the full sync pipeline: GL fetch -> entity fetch -> rules -> persist -> LLM."""

    def emit(msg: dict) -> None:
        if progress_callback is not None:
            try:
                progress_callback(msg)
            except Exception as cb_err:
                logger.warning("progress_callback error: %s", cb_err)

    # --- Default date range: 30 days back to today ---
    today = date.today()
    s_date = start_date or (today - timedelta(days=30)).isoformat()
    e_date = end_date or today.isoformat()

    start_dt = date.fromisoformat(s_date)
    end_dt = date.fromisoformat(e_date)

    # 1. Get OAuth tokens from Keychain
    emit({"phase": "auth", "message": "Retrieving OAuth tokens..."})
    try:
        access_token, refresh_token, realm_id = secrets.get_oauth_data()
    except Exception as e:
        logger.error(
            "Missing OAuth tokens in Keychain. Run the server and login at /auth/login first."
        )
        raise

    qbo_client = QboClient(realm_id=realm_id, access_token=access_token)

    # 2. Fetch the General Ledger report from QBO in 30-day chunks
    logger.info("Fetching QBO General Ledger for %s to %s", s_date, e_date)
    emit({"phase": "gl_fetch", "message": f"Fetching GL report {s_date} to {e_date}..."})

    chunks: list[tuple[str, str]] = []
    current_start = start_dt
    while current_start <= end_dt:
        chunk_end = current_start + timedelta(days=GL_CHUNK_DAYS)
        current_end = min(chunk_end, end_dt)
        chunks.append((current_start.isoformat(), current_end.isoformat()))
        current_start = current_end + timedelta(days=1)

    gl_entries: list[GlEntry] = []
    seen_tx: set[tuple[str, str]] = set()

    for chunk_start, chunk_end in chunks:
        logger.info("  -> Fetching GL chunk: %s to %s", chunk_start, chunk_end)
        emit({
            "phase": "gl_fetch",
            "message": f"Fetching GL chunk: {chunk_start} to {chunk_end}",
        })

        try:
            report = await qbo_client.fetch_general_ledger(chunk_start, chunk_end)
        except (AuthenticationError, Exception) as e:
            err_str = str(e)
            if "401" in err_str or "AuthenticationFailed" in err_str or isinstance(e, AuthenticationError):
                logger.warning("QBO Token expired. Refreshing...")
                emit({"phase": "gl_fetch", "message": "Token expired, refreshing..."})
                client_id, client_secret = secrets.get_client_credentials()
                new_tokens = await QboClient.refresh_oauth_token(
                    client_id, client_secret, refresh_token
                )
                secrets.store_oauth_data(
                    new_tokens.access_token, new_tokens.refresh_token, realm_id
                )
                qbo_client.update_access_token(new_tokens.access_token)
                logger.info("Token refreshed. Retrying GL chunk...")
                report = await qbo_client.fetch_general_ledger(chunk_start, chunk_end)
            else:
                raise

        chunk_entries: list[GlEntry] = []
        extract_gl_entries(report, "Unknown", chunk_entries, seen_tx)
        gl_entries.extend(chunk_entries)

    logger.info("Found %d unique classifiable transactions in the GL", len(gl_entries))
    emit({
        "phase": "gl_fetch",
        "message": f"Found {len(gl_entries)} unique classifiable transactions",
    })

    # 2b. Fetch active QBO Classes for the LLM prompt
    try:
        available_classes = await qbo_client.fetch_active_classes()
        logger.info("Fetched %d active classes from QBO", len(available_classes))
    except Exception as e:
        logger.warning("Failed to fetch QBO classes: %s. LLM will use empty list.", e)
        available_classes = []

    # 2c. Load classification rules from DB
    try:
        rules = await db.get_rules()
    except Exception as e:
        logger.warning(
            "Failed to load classification rules from DB: %s. Proceeding without rules.", e
        )
        rules = []
    if rules:
        logger.info("Loaded %d classification rules", len(rules))

    # 4. Fetch full entities concurrently with abort flag for 401 cascades
    abort_event = asyncio.Event()
    semaphore = asyncio.Semaphore(QBO_CONCURRENT_FETCHES)

    logger.info(
        "Fetching detailed entity data for %d items (concurrency=%d)...",
        len(gl_entries),
        QBO_CONCURRENT_FETCHES,
    )
    emit({
        "phase": "entity_fetch",
        "message": f"Fetching entity details for {len(gl_entries)} transactions...",
    })

    async def fetch_entity(entry: GlEntry) -> list[TransactionLine]:
        """Fetch a single entity and extract unclassified line items."""
        if abort_event.is_set():
            raise RuntimeError(
                f"Skipped {entry.api_type} id={entry.tx_id}: abort flag set due to earlier auth failure"
            )

        async with semaphore:
            # Artificial delay to prevent burst limit triggers per Intuit docs
            await asyncio.sleep(QBO_FETCH_DELAY_MS / 1000.0)

            try:
                entity = await qbo_client.fetch_transaction_by_id(
                    entry.api_type, entry.tx_id
                )
            except Exception as e:
                err_str = str(e)
                if "401" in err_str or "AuthenticationFailed" in err_str or isinstance(e, AuthenticationError):
                    logger.error(
                        "Token expired during concurrent fetch for %s id=%s. "
                        "Setting abort flag to skip remaining fetches.",
                        entry.api_type,
                        entry.tx_id,
                    )
                    abort_event.set()
                else:
                    logger.warning(
                        "Failed to fetch %s id=%s: %s", entry.api_type, entry.tx_id, e
                    )
                raise

            lines: list[TransactionLine] = []
            _id, sync_token, entity_name, header_memo = extract_header_info(entity)
            tx_date = entity.get("TxnDate") or entry.gl_date

            entity_lines = entity.get("Line")
            if not isinstance(entity_lines, list):
                return lines

            seen_line_ids: set[str] = set()
            for line in entity_lines:
                if not is_missing_class(line):
                    continue
                if line.get("DetailType") == "DescriptionOnly":
                    continue

                line_id = str(line.get("Id", "unknown"))

                # Prevent duplicates
                dup_key = (entry.tx_id, line_id)
                if dup_key in seen_line_ids:
                    continue
                seen_line_ids.add(dup_key)

                lines.append(
                    TransactionLine(
                        tx_id=entry.tx_id,
                        line_id=line_id,
                        sync_token=sync_token,
                        tx_type=entry.api_type,
                        tx_date=tx_date,
                        entity_name=entity_name,
                        header_memo=header_memo or entry.gl_memo,
                        line_description=line.get("Description"),
                        account=extract_account_name(line) or entry.gl_account,
                        account_type=entry.gl_account_type,
                        po_ref=None,
                        amount=line.get("Amount"),
                        suggested_class_ref=None,
                        llm_reasoning=None,
                        confidence_score=None,
                        status=TransactionStatus.Pending,
                    )
                )

            return lines

    # Launch all entity fetches concurrently
    tasks = [asyncio.create_task(fetch_entity(entry)) for entry in gl_entries]
    results = await asyncio.gather(*tasks, return_exceptions=True)

    total_attempted = len(results)
    fetch_failures = 0
    auth_aborted = False
    all_lines: list[TransactionLine] = []

    for result in results:
        if isinstance(result, BaseException):
            fetch_failures += 1
            err_str = str(result)
            if "abort flag" in err_str or "AuthenticationFailed" in err_str:
                auth_aborted = True
        else:
            all_lines.extend(result)

    if fetch_failures > 0:
        cause = (
            " CAUSE: OAuth token expired mid-sync. Re-run sync after refreshing tokens."
            if auth_aborted
            else ""
        )
        logger.error(
            "%d/%d entity fetches failed. These transactions will NOT appear in the review UI.%s",
            fetch_failures,
            total_attempted,
            cause,
        )
        emit({
            "phase": "entity_fetch",
            "message": f"{fetch_failures}/{total_attempted} entity fetches failed.{cause}",
        })

    # If auth failure caused a cascade, abort
    if auth_aborted and fetch_failures > total_attempted / 2:
        raise RuntimeError(
            f"Sync aborted: OAuth token expired mid-sync. "
            f"{fetch_failures}/{total_attempted} entity fetches failed. "
            f"Please refresh your token and re-run the sync."
        )

    logger.info("Extracted %d unclassified lines total.", len(all_lines))
    emit({
        "phase": "entity_fetch",
        "message": f"Extracted {len(all_lines)} unclassified lines",
    })

    # 5. Apply heuristic rules before LLM
    rule_applied_count = 0
    for line in all_lines:
        matched_class = find_matching_rule(line, rules)
        if matched_class is not None:
            line.suggested_class_ref = matched_class
            line.llm_reasoning = "Applied heuristic rule"
            line.confidence_score = 1.0
            rule_applied_count += 1

    if rule_applied_count > 0:
        logger.info(
            "%d lines classified by heuristic rules (skipping LLM)", rule_applied_count
        )
        emit({
            "phase": "rules",
            "message": f"{rule_applied_count} lines classified by heuristic rules",
        })

    # 6. Persist ALL lines immediately so they are visible in the review UI
    for line_item in all_lines:
        try:
            await db.insert_transaction(line_item)
        except Exception as e:
            logger.error(
                "DB insert failed [%s %s]: %s", line_item.tx_id, line_item.line_id, e
            )

    logger.info("%d lines persisted (Upserted safely).", len(all_lines))
    emit({
        "phase": "persist",
        "message": f"{len(all_lines)} lines persisted to database",
    })

    # 7. Send only non-rule-matched lines to LLM concurrently
    to_infer = [
        line
        for line in all_lines
        if not (
            line.confidence_score == 1.0
            and line.llm_reasoning == "Applied heuristic rule"
        )
    ]

    if to_infer:
        logger.info("Starting concurrent LLM inference for %d items...", len(to_infer))
        emit({
            "phase": "llm",
            "message": f"Running LLM inference for {len(to_infer)} items...",
        })

        llm_semaphore = asyncio.Semaphore(LLM_CONCURRENT_REQUESTS)
        completed_count = 0

        async def infer_line(line: TransactionLine) -> None:
            nonlocal completed_count
            async with llm_semaphore:
                logger.debug("Inferencing [%s %s]...", line.tx_id, line.line_id)
                try:
                    prediction = await llm_engine.predict_class(line, available_classes)
                except Exception as e:
                    logger.error(
                        "LLM failed for [%s %s]: %s", line.tx_id, line.line_id, e
                    )
                    return

                try:
                    await db.update_inference(
                        line.tx_id,
                        line.line_id,
                        prediction.class_ref,
                        prediction.reasoning,
                        prediction.confidence_score,
                    )
                    logger.info(
                        "=> [%s %s] class='%s' (%.0f%%)",
                        line.tx_id,
                        line.line_id,
                        prediction.class_ref,
                        prediction.confidence_score * 100.0,
                    )
                except Exception as e:
                    logger.error(
                        "Failed to update inference [%s %s]: %s",
                        line.tx_id,
                        line.line_id,
                        e,
                    )

                completed_count += 1
                if completed_count % 10 == 0:
                    emit({
                        "phase": "llm",
                        "message": f"LLM inference: {completed_count}/{len(to_infer)} complete",
                    })

        llm_tasks = [asyncio.create_task(infer_line(line)) for line in to_infer]
        await asyncio.gather(*llm_tasks, return_exceptions=True)

        emit({
            "phase": "llm",
            "message": f"LLM inference complete: {len(to_infer)} items processed",
        })

    logger.info("Sync complete.")
    emit({"phase": "done", "message": "Sync complete"})


# ---------------------------------------------------------------------------
# GL report parsing
# ---------------------------------------------------------------------------

def extract_gl_entries(
    node: dict[str, Any],
    current_account: str,
    entries: list[GlEntry],
    seen: set[tuple[str, str]],
) -> None:
    """Recursively extract transaction entries from QBO report rows,
    handling arbitrary nesting (sub-accounts)."""

    # 1. Check for account header in this section
    header_name = current_account
    header = node.get("Header")
    if isinstance(header, dict):
        col_data = header.get("ColData")
        if isinstance(col_data, list) and col_data:
            val = col_data[0].get("value") if isinstance(col_data[0], dict) else None
            if val:
                header_name = val

    # 2. Identify transaction rows (they have ColData)
    col_data = node.get("ColData")
    if isinstance(col_data, list):
        raw_tx_type = ""
        tx_id = ""

        if len(col_data) > 1 and isinstance(col_data[1], dict):
            raw_tx_type = col_data[1].get("value", "")
            tx_id = col_data[1].get("id", "")

        if tx_id and raw_tx_type:
            api_type = REPORT_TYPE_MAP.get(raw_tx_type)
            if api_type is not None:
                key = (api_type, tx_id)
                if key not in seen:
                    seen.add(key)

                    def _col_str(idx: int) -> str | None:
                        if idx < len(col_data) and isinstance(col_data[idx], dict):
                            v = col_data[idx].get("value", "")
                            return v if v else None
                        return None

                    def _col_float(idx: int) -> float | None:
                        s = _col_str(idx)
                        if s is None:
                            return None
                        try:
                            return float(s)
                        except (ValueError, TypeError):
                            return None

                    gl_date = _col_str(0)
                    gl_acc_name = _col_str(3)
                    gl_acc_type = _col_str(4)
                    gl_amount = _col_float(6)
                    gl_memo = _col_str(2)

                    entries.append(
                        GlEntry(
                            tx_id=tx_id,
                            api_type=api_type,
                            gl_account=gl_acc_name or header_name,
                            gl_account_type=gl_acc_type,
                            gl_date=gl_date,
                            gl_amount=gl_amount,
                            gl_memo=gl_memo,
                        )
                    )

    # 3. Recurse into children
    rows_container = node.get("Rows")
    if isinstance(rows_container, dict):
        row_list = rows_container.get("Row")
        if isinstance(row_list, list):
            for row in row_list:
                extract_gl_entries(row, header_name, entries, seen)


# ---------------------------------------------------------------------------
# Entity inspection helpers
# ---------------------------------------------------------------------------

def is_missing_class(line: dict[str, Any]) -> bool:
    """Return True when a line item has no ClassRef assigned."""
    for detail_key in (
        "AccountBasedExpenseLineDetail",
        "ItemBasedExpenseLineDetail",
        "JournalEntryLineDetail",
        "SalesItemLineDetail",
    ):
        detail = line.get(detail_key)
        if detail is not None:
            return detail.get("ClassRef") is None
    # No known detail type found
    return False


def extract_header_info(
    entity: dict[str, Any],
) -> tuple[str, str, str | None, str | None]:
    """Extract (id, sync_token, entity_name, memo) from a full entity."""
    entity_id = str(entity.get("Id", "unknown"))
    sync_token = str(entity.get("SyncToken", "0"))

    entity_name: str | None = None
    for ref_key in ("EntityRef", "VendorRef", "CustomerRef"):
        ref = entity.get(ref_key)
        if isinstance(ref, dict):
            name = ref.get("name")
            if name is not None:
                entity_name = str(name)
                break

    memo = entity.get("PrivateNote")
    if memo is not None:
        memo = str(memo)

    return entity_id, sync_token, entity_name, memo


def extract_account_name(line: dict[str, Any]) -> str | None:
    """Extract account name from a line detail object."""
    # Account-based lines
    acct = (
        line.get("AccountBasedExpenseLineDetail", {})
        .get("AccountRef", {})
        .get("name")
    )
    if acct:
        return str(acct)

    # Item-based lines (Lumber, Materials, etc.)
    item = (
        line.get("ItemBasedExpenseLineDetail", {}).get("ItemRef", {}).get("name")
    )
    if item:
        return f"Item: {item}"

    # Sales-based lines (Invoices, Sales Receipts)
    sales_item = (
        line.get("SalesItemLineDetail", {}).get("ItemRef", {}).get("name")
    )
    if sales_item:
        return f"Item: {sales_item}"

    # Journal Entry lines
    je_acct = (
        line.get("JournalEntryLineDetail", {}).get("AccountRef", {}).get("name")
    )
    if je_acct:
        return str(je_acct)

    return None


# ---------------------------------------------------------------------------
# Rule matching
# ---------------------------------------------------------------------------

def find_matching_rule(
    line: TransactionLine, rules: list[ClassificationRule]
) -> str | None:
    """Case-insensitive exact match on condition_field/condition_value.
    Returns the target_class if matched, else None."""
    _field_accessors: dict[str, Callable[[TransactionLine], str | None]] = {
        "account_type": lambda l: l.account_type,
        "account": lambda l: l.account,
        "tx_type": lambda l: l.tx_type,
        "entity_name": lambda l: l.entity_name,
    }

    for rule in rules:
        accessor = _field_accessors.get(rule.condition_field)
        field_value = accessor(line) if accessor else None

        if field_value is not None and field_value.lower() == rule.condition_value.lower():
            logger.info(
                "Rule match: %s = '%s' -> class '%s'",
                rule.condition_field,
                field_value,
                rule.target_class,
            )
            return rule.target_class

    return None
