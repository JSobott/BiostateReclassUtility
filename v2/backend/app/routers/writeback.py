"""Writeback to QBO — POST approved transactions, SSE progress, audit logs."""
import json
import logging
import uuid

from fastapi import APIRouter, BackgroundTasks, HTTPException, Request
from fastapi.responses import JSONResponse
from sse_starlette.sse import EventSourceResponse

from ..config import (
    CLASSIFIABLE_DETAIL_TYPES,
    ENTITY_KEY_MAP,
    REPORT_TYPE_MAP,
)
from ..models import AuditLog, TransactionStatus, WritebackRequest
from ..qbo_client import QboClient
from ..secrets import get_oauth_data, store_oauth_data

logger = logging.getLogger(__name__)

router = APIRouter()


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
def _get_entity_type(tx_type: str) -> str | None:
    """Map a human-readable tx_type to the QBO API entity name."""
    return REPORT_TYPE_MAP.get(tx_type)


def _get_entity_key(entity_type: str) -> str | None:
    """Map a QBO API entity name to the JSON response key."""
    return ENTITY_KEY_MAP.get(entity_type)


def _mutate_class_ref(line: dict, new_class_ref: str) -> bool:
    """Inject or update ClassRef in a line's detail block. Returns True if mutated."""
    for detail_type in CLASSIFIABLE_DETAIL_TYPES:
        detail = line.get(detail_type)
        if detail is not None:
            detail["ClassRef"] = {
                "value": new_class_ref.split(":")[0] if ":" in new_class_ref else new_class_ref,
                "name": new_class_ref.split(":")[1] if ":" in new_class_ref else new_class_ref,
            }
            return True
    return False


async def _perform_writeback(
    db,
    broadcaster,
    dry_run: bool,
) -> None:
    """Background task: fetch approved transactions, mutate, and POST to QBO."""
    batch_id = str(uuid.uuid4())
    try:
        await broadcaster.publish({"status": "started", "batch_id": batch_id})

        approved = await db.get_approved_or_validated_transactions()
        if not approved:
            await broadcaster.publish({"status": "done", "batch_id": batch_id, "total": 0, "posted": 0, "failed": 0})
            return

        # Group lines by tx_id
        tx_groups: dict[str, list] = {}
        for tx in approved:
            tx_groups.setdefault(tx.tx_id, []).append(tx)

        total = len(tx_groups)
        posted = 0
        failed = 0

        access_token, refresh_token, realm_id = get_oauth_data()
        client = QboClient(access_token, refresh_token, realm_id)

        for idx, (tx_id, lines) in enumerate(tx_groups.items()):
            first_line = lines[0]
            entity_type = _get_entity_type(first_line.tx_type)
            if not entity_type:
                logger.error("Unknown tx_type '%s' for tx_id %s", first_line.tx_type, tx_id)
                failed += 1
                for ln in lines:
                    await db.update_status(ln.tx_id, ln.line_id, TransactionStatus.Failed)
                await broadcaster.publish({
                    "status": "progress",
                    "batch_id": batch_id,
                    "current": idx + 1,
                    "total": total,
                    "tx_id": tx_id,
                    "result": "failed",
                    "reason": f"Unknown tx_type: {first_line.tx_type}",
                })
                continue

            entity_key = _get_entity_key(entity_type)

            try:
                # Fetch latest entity from QBO
                entity_json = await client.fetch_entity(entity_type, tx_id)
                if not entity_json or entity_key not in entity_json:
                    raise ValueError(f"Entity key '{entity_key}' not found in response")

                entity = entity_json[entity_key]

                # Mutate ClassRef in matching lines
                qbo_lines = entity.get("Line", [])
                for tx_line in lines:
                    for qbo_line in qbo_lines:
                        qbo_line_id = str(qbo_line.get("Id", ""))
                        if qbo_line_id == tx_line.line_id and tx_line.suggested_class_ref:
                            _mutate_class_ref(qbo_line, tx_line.suggested_class_ref)

                # Set sparse update
                entity["sparse"] = True

                request_json = json.dumps(entity)

                if dry_run:
                    # Validate only — don't actually post
                    response_json = json.dumps({"dry_run": True, "would_post": entity_type, "tx_id": tx_id})
                    for ln in lines:
                        await db.update_status(ln.tx_id, ln.line_id, TransactionStatus.Validated)
                    await db.log_writeback_audit(batch_id, request_json, response_json, "validated")
                    posted += 1
                else:
                    # POST to QBO
                    try:
                        resp = await client.update_entity(entity_type, entity)
                        response_json = json.dumps(resp) if resp else "{}"
                        for ln in lines:
                            await db.update_status(ln.tx_id, ln.line_id, TransactionStatus.Posted)
                        await db.log_writeback_audit(batch_id, request_json, response_json, "posted")
                        posted += 1
                    except Exception as post_err:
                        # Token refresh on 401
                        if "401" in str(post_err):
                            try:
                                new_tokens = await client.refresh_tokens()
                                store_oauth_data(new_tokens["access_token"], new_tokens["refresh_token"], realm_id)
                                client = QboClient(new_tokens["access_token"], new_tokens["refresh_token"], realm_id)
                                resp = await client.update_entity(entity_type, entity)
                                response_json = json.dumps(resp) if resp else "{}"
                                for ln in lines:
                                    await db.update_status(ln.tx_id, ln.line_id, TransactionStatus.Posted)
                                await db.log_writeback_audit(batch_id, request_json, response_json, "posted")
                                posted += 1
                                continue
                            except Exception as refresh_err:
                                logger.error("Token refresh failed: %s", refresh_err)

                        raise post_err

            except Exception as e:
                logger.error("Writeback failed for tx_id %s: %s", tx_id, e)
                failed += 1
                for ln in lines:
                    await db.update_status(ln.tx_id, ln.line_id, TransactionStatus.Failed)
                await db.log_writeback_audit(
                    batch_id,
                    json.dumps({"tx_id": tx_id, "entity_type": entity_type}),
                    json.dumps({"error": str(e)}),
                    "failed",
                )

            await broadcaster.publish({
                "status": "progress",
                "batch_id": batch_id,
                "current": idx + 1,
                "total": total,
                "tx_id": tx_id,
                "result": "ok" if failed == 0 or (posted + failed) <= idx else "failed",
            })

        await broadcaster.publish({
            "status": "done",
            "batch_id": batch_id,
            "total": total,
            "posted": posted,
            "failed": failed,
        })

    except Exception as e:
        logger.error("Writeback batch failed: %s", e)
        await broadcaster.publish({
            "status": "error",
            "batch_id": batch_id,
            "error": str(e),
        })


# ---------------------------------------------------------------------------
# Routes
# ---------------------------------------------------------------------------
@router.post("/api/writeback", status_code=202)
async def start_writeback(
    request: Request,
    body: WritebackRequest,
    background_tasks: BackgroundTasks,
):
    """Kick off a writeback to QBO as a background task."""
    db = request.app.state.db
    broadcaster = request.app.state.broadcaster
    background_tasks.add_task(_perform_writeback, db, broadcaster, body.dry_run)
    return {"status": "accepted", "dry_run": body.dry_run}


@router.get("/api/writeback-progress")
async def writeback_progress(request: Request):
    """SSE stream for writeback progress updates."""
    broadcaster = request.app.state.broadcaster
    queue = broadcaster.subscribe()

    async def event_generator():
        try:
            while True:
                message = await queue.get()
                yield {"event": "message", "data": json.dumps(message)}
                if message.get("status") in ("done", "error"):
                    break
        finally:
            broadcaster.unsubscribe(queue)

    return EventSourceResponse(event_generator())


@router.get("/api/audit-logs", response_model=list[AuditLog])
async def get_audit_logs(request: Request):
    """Return all writeback audit log entries."""
    db = request.app.state.db
    try:
        return await db.get_audit_logs()
    except Exception as e:
        logger.error("Failed to fetch audit logs: %s", e)
        raise HTTPException(status_code=500, detail="Failed to fetch audit logs")
