"""Sync endpoint — trigger QBO transaction sync with SSE progress."""
import json
import logging

from fastapi import APIRouter, BackgroundTasks, HTTPException, Request
from sse_starlette.sse import EventSourceResponse

from ..models import SyncRequest
from ..qbo_client import QboClient
from ..secrets import get_oauth_data, store_oauth_data, KeychainError

logger = logging.getLogger(__name__)

router = APIRouter()


async def _perform_sync(
    db,
    broadcaster,
    start_date: str | None,
    end_date: str | None,
) -> None:
    """Background task: fetch transactions from QBO, classify, and store."""
    try:
        await broadcaster.publish({"status": "started", "phase": "sync"})

        # Get QBO client
        try:
            access_token, refresh_token, realm_id = get_oauth_data()
        except KeychainError:
            await broadcaster.publish({
                "status": "error",
                "phase": "sync",
                "error": "Not authenticated. Please connect to QuickBooks first.",
            })
            return

        client = QboClient(access_token, refresh_token, realm_id)

        # Fetch transactions from the GL report
        await broadcaster.publish({"status": "progress", "phase": "fetch", "message": "Fetching GL data from QBO..."})

        try:
            transactions = await client.fetch_gl_transactions(start_date, end_date)
        except Exception as e:
            if "401" in str(e):
                logger.info("Access token expired during sync, refreshing...")
                try:
                    new_tokens = await client.refresh_tokens()
                    store_oauth_data(new_tokens["access_token"], new_tokens["refresh_token"], realm_id)
                    client = QboClient(new_tokens["access_token"], new_tokens["refresh_token"], realm_id)
                    transactions = await client.fetch_gl_transactions(start_date, end_date)
                except Exception as refresh_err:
                    await broadcaster.publish({
                        "status": "error",
                        "phase": "sync",
                        "error": f"Token refresh failed: {refresh_err}",
                    })
                    return
            else:
                raise

        total = len(transactions)
        await broadcaster.publish({
            "status": "progress",
            "phase": "store",
            "message": f"Storing {total} transaction lines...",
            "total": total,
        })

        # Store each transaction line
        for idx, tx in enumerate(transactions):
            await db.insert_transaction(tx)
            if (idx + 1) % 50 == 0 or idx + 1 == total:
                await broadcaster.publish({
                    "status": "progress",
                    "phase": "store",
                    "current": idx + 1,
                    "total": total,
                })

        await broadcaster.publish({
            "status": "done",
            "phase": "sync",
            "total": total,
        })

    except Exception as e:
        logger.error("Sync failed: %s", e)
        await broadcaster.publish({
            "status": "error",
            "phase": "sync",
            "error": str(e),
        })


@router.post("/api/sync", status_code=202)
async def start_sync(
    request: Request,
    body: SyncRequest,
    background_tasks: BackgroundTasks,
):
    """Kick off a QBO sync as a background task."""
    db = request.app.state.db
    broadcaster = request.app.state.broadcaster
    background_tasks.add_task(_perform_sync, db, broadcaster, body.start_date, body.end_date)
    return {"status": "accepted"}


@router.get("/api/sync-progress")
async def sync_progress(request: Request):
    """SSE stream for sync progress updates."""
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
