"""Transaction listing, grouping, approval, and reset endpoints."""
import logging

from fastapi import APIRouter, HTTPException, Request

from ..models import (
    ApprovalRequest,
    BatchApproveRequest,
    ResetPendingRequest,
    TransactionBucket,
    TransactionLine,
    TransactionStatus,
)

logger = logging.getLogger(__name__)

router = APIRouter()


@router.get("/api/pending", response_model=list[TransactionLine])
async def get_pending(request: Request):
    """Return all transaction lines with status Pending."""
    db = request.app.state.db
    try:
        return await db.get_pending_transactions()
    except Exception as e:
        logger.error("Failed to fetch pending transactions: %s", e)
        raise HTTPException(status_code=500, detail="Failed to fetch pending transactions")


@router.get("/api/groups", response_model=list[TransactionBucket])
async def get_groups(request: Request, status: str = "Pending"):
    """Return transactions grouped by account / entity / suggested class."""
    db = request.app.state.db
    try:
        return await db.get_grouped_transactions(status)
    except Exception as e:
        logger.error("Failed to fetch grouped transactions: %s", e)
        raise HTTPException(status_code=500, detail="Failed to fetch grouped transactions")


@router.get("/api/group-items", response_model=list[TransactionLine])
async def get_group_items(
    request: Request,
    status: str,
    account: str,
    entity_name: str,
    suggested_class: str,
):
    """Return individual lines within a specific group."""
    db = request.app.state.db
    try:
        return await db.get_group_items(status, account, entity_name, suggested_class)
    except Exception as e:
        logger.error("Failed to fetch group items: %s", e)
        raise HTTPException(status_code=500, detail="Failed to fetch group items")


@router.post("/api/batch-approve")
async def batch_approve(request: Request, body: BatchApproveRequest):
    """Approve all pending lines in a group, optionally overriding the class."""
    db = request.app.state.db
    try:
        count = await db.batch_approve_group(
            body.account, body.entity_name, body.current_class, body.override_class
        )
        return {"status": "ok", "count": count}
    except Exception as e:
        logger.error("Batch approve failed: %s", e)
        raise HTTPException(status_code=500, detail="Batch approve failed")


@router.post("/api/approve")
async def approve_single(request: Request, body: ApprovalRequest):
    """Approve a single transaction line."""
    db = request.app.state.db

    # Set the suggested class if provided
    if body.suggested_class_ref:
        await db.update_inference(
            body.tx_id, body.line_id, body.suggested_class_ref, "User approval", 1.0
        )

    updated = await db.update_status(body.tx_id, body.line_id, TransactionStatus.Approved)
    if updated == 0:
        logger.warning("Approve had no effect for %s:%s — may already be approved or missing", body.tx_id, body.line_id)
        raise HTTPException(
            status_code=409,
            detail="Transaction not in a state that allows approval",
        )
    return {"status": "ok"}


@router.post("/api/reset-pending")
async def reset_pending(request: Request, body: ResetPendingRequest):
    """Reset failed lines in a group back to Pending."""
    db = request.app.state.db
    try:
        count = await db.reset_failed_group_to_pending(
            body.account, body.entity_name, body.current_class
        )
        return {"status": "ok", "count": count}
    except Exception as e:
        logger.error("Reset pending failed: %s", e)
        raise HTTPException(status_code=500, detail="Reset pending failed")
